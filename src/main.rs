#![feature(int_roundings)]

mod structs;
use crate::structs::{
    BlockGroupDescriptor, DirectoryEntry, Inode, Superblock, TypeIndicator, TypePerm,
};
use null_terminated::NulStr;
use rustyline::{DefaultEditor, Result};
use std::cmp;
use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::mem;
use std::slice;
use std::str;
use uuid::Uuid;
use zerocopy::AsBytes;
use zerocopy::ByteSlice;

#[repr(C)]
#[derive(Debug)]
pub struct Ext2 {
    pub superblock: &'static Superblock,
    pub block_groups: &'static [BlockGroupDescriptor],
    pub blocks: Vec<&'static [u8]>,
    pub block_size: usize,
    pub uuid: Uuid,
    pub block_offset: usize, // <- our "device data" actually starts at this index'th block of the device
                             // so we have to subtract this number before indexing blocks[]
}

const EXT2_MAGIC: u16 = 0xef53;
const EXT2_START_OF_SUPERBLOCK: usize = 1024;
const EXT2_END_OF_SUPERBLOCK: usize = 2048;

impl Ext2 {
    pub fn new<B: ByteSlice + std::fmt::Debug>(device_bytes: B, start_addr: usize) -> Ext2 {
        // https://wiki.osdev.org/Ext2#Superblock
        // parse into Ext2 struct - without copying

        // the superblock goes from bytes 1024 -> 2047
        let header_body_bytes = device_bytes.split_at(EXT2_END_OF_SUPERBLOCK);

        let superblock = unsafe {
            &*(header_body_bytes
                .0
                .split_at(EXT2_START_OF_SUPERBLOCK)
                .1
                .as_ptr() as *const Superblock)
        };
        assert_eq!(superblock.magic, EXT2_MAGIC);
        // at this point, we strongly suspect these bytes are indeed an ext2 filesystem

        println!("superblock:\n{:?}", superblock);
        println!("size of Inode struct: {}", mem::size_of::<Inode>());

        let block_group_count = superblock
            .blocks_count
            .div_ceil(superblock.blocks_per_group) as usize;

        let block_size: usize = 1024 << superblock.log_block_size;
        println!(
            "there are {} block groups and block_size = {}",
            block_group_count, block_size
        );
        let block_groups_rest_bytes = header_body_bytes.1.split_at(block_size);

        let block_groups = unsafe {
            std::slice::from_raw_parts(
                block_groups_rest_bytes.0.as_ptr() as *const BlockGroupDescriptor,
                block_group_count,
            )
        };

        println!("block group 0: {:?}", block_groups[0]);

        let blocks = unsafe {
            std::slice::from_raw_parts(
                block_groups_rest_bytes.1.as_ptr() as *mut u8,
                // would rather use: device_bytes.as_ptr(),
                superblock.blocks_count as usize * block_size,
            )
        }
        .chunks(block_size)
        .collect::<Vec<_>>();

        let offset_bytes = (blocks[0].as_ptr() as usize) - start_addr;
        let block_offset = offset_bytes / block_size;
        let uuid = Uuid::from_bytes(superblock.fs_id);
        Ext2 {
            superblock,
            block_groups,
            blocks,
            block_size,
            uuid,
            block_offset,
        }
    }

    // given a (1-indexed) inode number, return that #'s inode structure
    pub fn get_inode(&self, inode: usize) -> &Inode {
        let group: usize = (inode - 1) / self.superblock.inodes_per_group as usize;
        let index: usize = (inode - 1) % self.superblock.inodes_per_group as usize;

        // println!("in get_inode, inode num = {}, index = {}, group = {}", inode, index, group);
        let inode_table_block =
            (self.block_groups[group].inode_table_block) as usize - self.block_offset;
        // println!("in get_inode, block number of inode table {}", inode_table_block);
        let inode_table = unsafe {
            std::slice::from_raw_parts(
                self.blocks[inode_table_block].as_ptr() as *const Inode,
                self.superblock.inodes_per_group as usize,
            )
        };
        let node = &inode_table[index];
        // println!("inode permissions: {}", node.type_perm.bits());
        // probably want a Vec of BlockGroups in our Ext structure so we don't have to slice each time,
        // but this works for now.
        // println!("{:?}", inode_table);
        &inode_table[index]
    }

    pub fn read_dir_entry_block(
        &self,
        contiguous_data: &mut Vec<u8>,
        direct_pointer: *const u8,
        whole_size: u64,
        bytes_read: u64,
    ) -> std::io::Result<isize> {
        let bytes_to_read = cmp::min(self.block_size, (whole_size as usize - bytes_read as usize));
        // read all the bytes in that block
        let new_data = unsafe { slice::from_raw_parts(direct_pointer, bytes_to_read) };
        contiguous_data.extend_from_slice(new_data);
        Ok(bytes_to_read as isize)
    }

    pub fn contiguous_data_from_dir_inode(&self, inode: usize) -> std::io::Result<Vec<u8>> {
        let root = self.get_inode(inode);
        if root.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "inode is not a directory",
            ));
        }

        let whole_size: u64 = ((root.size_high as u64) << 32) + root.size_low as u64;
        let mut contiguous_data: Vec<u8> = Vec::new();
        let mut i = 0;
        let mut bytes_read: isize = 0;
        // get all the direct pointer blocks
        while i < 12 && bytes_read < whole_size as isize {
            let entry_ptr =
                self.blocks[root.direct_pointer[i] as usize - self.block_offset].as_ptr();
            let ret: isize = match self.read_dir_entry_block(
                &mut contiguous_data,
                entry_ptr,
                whole_size,
                bytes_read as u64,
            ) {
                Ok(dir_listing) => dir_listing,
                Err(_) => {
                    panic!("OOps");
                }
            };
            bytes_read += ret;
            i += 1;
        }
        for i in (0..contiguous_data.len()).rev() {
            if contiguous_data[i] != 0 {
                contiguous_data.truncate(i + 1);
                // println!("contiguous data after trim: {:?}", contiguous_data);
                return Ok(contiguous_data);
            }
        }

        return Ok(contiguous_data);
    }

    pub fn read_dir_inode(&self, inode: usize) -> std::io::Result<Vec<(usize, &NulStr)>> {
        let mut ret_vec = Vec::new();
        // get data from inode data as a contiguous vector
        let contiguous_data = match self.contiguous_data_from_dir_inode(inode) {
            Ok(data_vector) => data_vector,
            Err(_) => panic!("Whoopsies"),
        };

        let root = self.get_inode(inode);
        if root.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "inode is not a directory",
            ));
        }

        let whole_size: u64 = ((root.size_high as u64) << 32) + root.size_low as u64;

        let data_ptr = contiguous_data.as_ptr();
        let mut byte_offset: isize = 0;
        while byte_offset < contiguous_data.len() as isize {
            let directory = unsafe { &*(data_ptr.offset(byte_offset) as *const DirectoryEntry) };
            byte_offset += directory.entry_size as isize;
            println!("entry name: {}", &directory.name);
            println!("entry size: {}", directory.entry_size);
            ret_vec.push((directory.inode as usize, &directory.name));
        }
        Ok(ret_vec)
    }

    pub fn write_dir_entry_block(
        &self,
        contiguous_data: &mut Vec<u8>,
        direct_pointer: *mut u8,
        whole_size: u64,
        bytes_written: u64,
    ) -> std::io::Result<isize> {
        let bytes_to_write = cmp::min(
            self.block_size,
            whole_size as usize - bytes_written as usize,
        );

        let data_ptr = (contiguous_data as *const Vec<u8>) as *const u8;
        // get subarray of data to be written back
        let vec_to_write = unsafe {
            std::slice::from_raw_parts(data_ptr.offset(bytes_written as isize), bytes_to_write)
        };

        // then write vec_to_write to self.blocks
        for i in 0..vec_to_write.len() {
            unsafe {
                direct_pointer
                    .offset(i as isize)
                    .write_bytes(contiguous_data[(bytes_written + i as u64) as usize], 1)
            }
        }

        Ok(bytes_to_write as isize)
    }

    pub fn write_dir_inode(
        &self,
        inode: usize,
        data: &mut Vec<u8>,
        new_entry_size: u16,
    ) -> std::io::Result<()> {
        let root = self.get_inode(inode);
        if root.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "inode is not a directory",
            ));
        }

        let whole_size: u64 = data.len() as u64;

        let mut i = 0;
        let mut bytes_written: isize = 0;
        // write to all the direct pointer blocks
        while i < 12 && bytes_written < whole_size as isize && root.direct_pointer[i] != 0 {
            let entry_ptr = self.blocks[root.direct_pointer[i] as usize - self.block_offset];
            let ret: isize = match self.write_dir_entry_block(
                data,
                entry_ptr.as_ptr() as *mut u8,
                whole_size,
                bytes_written as u64,
            ) {
                Ok(dir_listing) => dir_listing,
                Err(_) => {
                    panic!("Opps");
                }
            };
            bytes_written += ret;
            i += 1;
        }

        assert!(bytes_written as u64 == whole_size);
        return Ok(());
    }

    pub fn insert_dir_entry(&self, inode: usize, name: &str) -> std::io::Result<()> {
        // read in data from directory entry
        let mut contiguous_data = match self.contiguous_data_from_dir_inode(inode) {
            Ok(data_vector) => data_vector,
            Err(_) => panic!("Whoopsies"),
        };

        // update second to last directory entry's size
        let data_ptr = contiguous_data.as_ptr() as *mut u8;
        let mut byte_offset: isize = 0;
        // find second to last directory entry
        let mut num_dir_entry = 0;
        let mut last_dir_entry_name_length = 0;
        while byte_offset < contiguous_data.len() as isize {
            let directory = unsafe { &*(data_ptr.offset(byte_offset) as *const DirectoryEntry) };
            byte_offset += directory.entry_size as isize;
            println!("entry name: {}", &directory.name);
            println!("entry size0: {}", directory.entry_size);
            last_dir_entry_name_length = directory.name_length;
            num_dir_entry += 1;
        }
        // update entry size for second to last directory entry
        let mut i = 0;
        byte_offset = 0;
        let mut entry_size = last_dir_entry_name_length as usize;
        while byte_offset < contiguous_data.len() as isize {
            i += 1;
            if i == num_dir_entry {
                entry_size += (mem::size_of::<u32>()
                    + mem::size_of::<u16>()
                    + mem::size_of::<u8>()
                    + mem::size_of::<TypeIndicator>()
                    + 1);
                unsafe {
                    data_ptr
                        .offset(byte_offset + 4)
                        .write_bytes(entry_size.as_bytes()[0], 1)
                };
                unsafe {
                    data_ptr
                        .offset(byte_offset + 5)
                        .write_bytes(entry_size.as_bytes()[1], 1)
                };
            }
            let directory = unsafe { &*(data_ptr.offset(byte_offset) as *const DirectoryEntry) };
            byte_offset += directory.entry_size as isize;
        }

        // add the new directory entry to the end as bytes
        contiguous_data.extend_from_slice((inode as u32).as_bytes());
        // calculate size of new entry
        let entry_size = mem::size_of::<u32>()
            + mem::size_of::<u16>()
            + mem::size_of::<u8>()
            + mem::size_of::<TypeIndicator>()
            + name.len()
            + 1;

        contiguous_data.extend_from_slice((entry_size as u16).as_bytes());
        let name_size = name.len() + 1;
        contiguous_data.extend((name_size as u8).as_bytes());
        // type is directory entry
        contiguous_data.push(2);
        contiguous_data.extend_from_slice(name.as_bytes());
        let null = "\0";
        contiguous_data.extend_from_slice(null.as_bytes());
        let root = self.get_inode(inode);
        if root.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "inode is not a directory",
            ));
        }

        // write data back out
        self.write_dir_inode(inode, &mut contiguous_data, entry_size as u16)
            .expect("write_dir_inode fails");

        // make the entry size correct
        // add to inode table
        return Ok(());
    }

    pub fn follow_path(&self, path: &str, dirs: Vec<(usize, &NulStr)>) -> Option<usize> {
        let mut candidate_directories: VecDeque<&str> = path.split('/').collect();
        let mut dirs: Vec<(usize, &NulStr)> = dirs;
        let mut possible_inode: usize = 2;
        // directory where the call is made from
        let initial_dir = dirs[0].0;
        let mut candidate = None;

        while candidate_directories.len() > 0 {
            candidate = Some(candidate_directories.pop_front().unwrap());
            let mut found = false;
            // find next directory
            for dir in &dirs {
                if dir.1.to_string().eq(candidate.unwrap()) {
                    found = true;
                    // update inode of current directory
                    possible_inode = dir.0;
                    break;
                }
            }
            if !found {
                println!("unable to locate {}", candidate.unwrap());
                return None;
            } else {
                let inode = self.get_inode(possible_inode);
                // check type permission of inode, for last inode can be not a directory (for cat)
                if inode.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY
                    && candidate_directories.len() != 0
                {
                    println!("not a directory: {}", candidate.unwrap());
                    return Some(initial_dir);
                } else {
                    if candidate_directories.len() > 0 {
                        // update current directory
                        dirs = match self.read_dir_inode(possible_inode) {
                            Ok(dir_listing) => dir_listing,
                            Err(_) => {
                                println!("unable to read directory");
                                break;
                            }
                        }
                    }
                }
            }
        }
        return Some(possible_inode);
    }

    pub fn read_file_inode(&self, inode: usize) -> std::io::Result<Vec<&NulStr>> {
        let mut ret = Vec::new();
        let root = self.get_inode(inode);
        // make sure we are reading a file
        if root.type_perm & TypePerm::FILE != TypePerm::FILE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "inode is not a file",
            ));
        }

        // go through all the direct pointers
        for cont in root.direct_pointer {
            // <- todo, support large directories
            // if this is 0, then that means the pointer is nullptr and we are done
            if cont != 0 {
                let directory = unsafe {
                    &*(self.blocks[cont as usize - self.block_offset].as_ptr() as *const NulStr)
                };
                ret.push(directory);
            }
        }
        Ok(ret)
    }

    pub fn ls(&self, dirs: Vec<(usize, &NulStr)>, command: String) -> Option<()> {
        let elts: Vec<&str> = command.split(' ').collect();
        if elts.len() == 1 {
            for dir in &dirs {
                print!("{}\t", dir.1);
            }
            println!();
        } else {
            let paths = elts[1];
            let inode = self.follow_path(paths, dirs);
            if inode.is_none() {
                println!("unable to follow path");
            }
            let possible_inode = self.get_inode(inode.unwrap());
            if possible_inode.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
                println!("not a directory: {}", paths);
            }
            // get directories for
            let dirs_to_show: Option<Vec<(usize, &NulStr)>> =
                match self.read_dir_inode(inode.unwrap()) {
                    Ok(dir_listing) => Some(dir_listing),
                    Err(_) => None,
                };
            if dirs_to_show.is_none() {
                println!("unable to read directory in ls");
                return None;
            }
            for dir in &dirs_to_show.unwrap() {
                print!("{}\t", dir.1);
            }
            println!();
        }
        return Some(());
    }

    pub fn cd(&self, dirs: Vec<(usize, &NulStr)>, command: String) -> Option<usize> {
        // `cd` with no arguments, cd goes back to root
        // `cd dir_name` moves cwd to that directory
        let elts: Vec<&str> = command.split(' ').collect();
        if elts.len() == 1 {
            return Some(2);
        } else {
            let paths = elts[1];
            let inode = self.follow_path(paths, dirs);
            if inode.is_none() {
                println!("cd: unable to find directory: {}", paths);
                return None;
            } else {
                let possible_inode = self.get_inode(inode.unwrap());
                // let possible_inode = ext2.get_inode(inode);
                if possible_inode.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
                    println!("not a directory: {}", paths);
                }
                return Some(inode.unwrap());
            }
        }
    }

    pub fn mkdir(&self, dirs: Vec<(usize, &NulStr)>, inode: usize, command: String) -> Option<()> {
        // `mkdir childname`
        // create a directory with the given name, add a link to cwd
        // consider supporting `-p path/to_file` to create a path of directories
        let elts: Vec<&str> = command.split(' ').collect();
        if elts.len() == 1 {
            print!("must pass file to mkdir");
        }
        let name = elts[1];

        self.insert_dir_entry(inode, name)
            .expect("insert_dir_entry failed");
        Some(())
    }

    pub fn cat(&self, dirs: Vec<(usize, &NulStr)>, command: String) -> Option<()> {
        // `cat filename`
        // print the contents of filename to stdout
        // if it's a directory, print a nice error
        let elts: Vec<&str> = command.split(' ').collect();
        if elts.len() == 1 {
            print!("must pass file to show");
        } else {
            let paths = elts[1];
            // get inode of potential file
            let possible_inode = self.follow_path(paths, dirs);
            if possible_inode.is_none() {
                println!("unable to follow path");
            } else {
                let inode = self.get_inode(possible_inode.unwrap());
                if inode.type_perm & TypePerm::FILE != TypePerm::FILE {
                    println!("not a file: {}", paths);
                    return None;
                } else {
                    let file_contents: Vec<&NulStr> =
                        match self.read_file_inode(possible_inode.unwrap()) {
                            Ok(file_data) => file_data,
                            Err(_) => {
                                println!("unable to cat file: {}", paths);
                                return None;
                            }
                        };

                    for cont in &file_contents {
                        print!("{}\t", cont);
                    }
                }
            }
        }
        return Some(());
    }

    pub fn rm(&self, dirs: Vec<(usize, &NulStr)>, command: String) -> Option<()> {
        // `rm target`
        // unlink a file or empty directory
        println!("rm not yet implemented");
        return None;
    }

    pub fn mount(&self, dirs: Vec<(usize, &NulStr)>, command: String) -> Option<()> {
        // `mount host_filename mountpoint`
        // mount an ext2 filesystem over an existing empty directory
        println!("mount not yet implemented");
        return None;
    }

    pub fn link(
        &self,
        current_working_inode: usize,
        dirs: Vec<(usize, &NulStr)>,
        command: String,
    ) -> Option<()> {
        // `link arg_1 arg_2`
        // create a hard link from arg_1 to arg_2
        // consider what to do if arg2 does- or does-not end in "/"
        // and/or if arg2 is an existing directory name

        let elts: Vec<&str> = command.split(' ').collect();
        if elts.len() != 3 {
            println!("usage: link arg_1 arg_2 ...");
            return None;
        }

        let arg_1 = elts[1];
        // for right now assume that arg_2 is not a path
        let arg_2 = elts[2];
        // first make sure that arg_1 does in fact exist
        let inode_number = self.follow_path(arg_1, dirs);
        if inode_number.is_none() {
            println!("unable to follow path to arg_1");
            return None;
        }
        // in parent directory of arg_1 we need to make a new directory entry with arg_1 that corresponds to the same inode number as arg_2
        let inode = self.get_inode(inode_number.unwrap());
        let parent_directory = self.read_dir_inode(current_working_inode);
        let test_string = parent_directory.unwrap().pop().unwrap().1;

        let mut entry_type: TypeIndicator;
        if inode.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
            entry_type = TypeIndicator::Directory;
        } else if inode.type_perm & TypePerm::FILE != TypePerm::FILE {
            entry_type = TypeIndicator::Regular;
        }
        println!("link not yet implemented");
        return None;
    }
}

impl fmt::Debug for Inode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.size_low == 0 && self.size_high == 0 {
            f.debug_struct("").finish()
        } else {
            f.debug_struct("Inode")
                .field("type_perm", &self.type_perm)
                .field("size_low", &self.size_low)
                .field("direct_pointers", &self.direct_pointer)
                .field("indirect_pointer", &self.indirect_pointer)
                .finish()
        }
    }
}
fn main() -> Result<()> {
    // load disk at runtime rather than compile time
    let disk = fs::read("myfs.ext2").expect("Couldn't find FS");
    // let disk = include_bytes!("../largefs.ext2");
    let start_addr: usize = disk.as_ptr() as usize;
    let ext2 = Ext2::new(&disk[..], start_addr);

    let mut current_working_inode: usize = 2;

    let mut rl = DefaultEditor::new()?;
    loop {
        // fetch the children of the current working directory
        let dirs = match ext2.read_dir_inode(current_working_inode) {
            Ok(dir_listing) => dir_listing,
            Err(_) => {
                println!("unable to read cwd");
                break;
            }
        };

        let buffer = rl.readline(":> ");
        if let Ok(line) = buffer {
            if line.starts_with("ls") {
                let success = ext2.ls(dirs, line);
                if success.is_none() {
                    println!("unable to read directory in ls");
                }
            } else if line.starts_with("cd") {
                let possible_working_inode = ext2.cd(dirs, line);
                if possible_working_inode.is_none() {
                    println!("unable to read directory in cd");
                } else {
                    current_working_inode = possible_working_inode.unwrap();
                }
            } else if line.starts_with("mkdir") {
                let success = ext2.mkdir(dirs, current_working_inode, line);
                if success.is_none() {
                    println!("unable to create directory in mkdir");
                }
            } else if line.starts_with("cat") {
                let success = ext2.cat(dirs, line);
                if success.is_none() {
                    println!("unable to cat file");
                }
                // println!("cat not yet implemented");
            } else if line.starts_with("rm") {
                let success = ext2.rm(dirs, line);
                if success.is_none() {
                    println!("unable to remove directory in rm");
                }
            } else if line.starts_with("mount") {
                let success = ext2.mount(dirs, line);
                if success.is_none() {
                    println!("unable to mount directory in rm");
                }
            } else if line.starts_with("link") {
                let success = ext2.link(current_working_inode, dirs, line);
                if success.is_none() {
                    println!("link to mount directory in rm");
                }
            } else if line.starts_with("quit") || line.starts_with("exit") {
                break;
            }
        } else {
            println!("bye!");
            break;
        }
    }
    Ok(())
}
