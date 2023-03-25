#![feature(int_roundings)]

mod structs;
use crate::structs::{BlockGroupDescriptor, DirectoryEntry, Inode, Superblock, TypePerm};
use null_terminated::NulStr;
use rustyline::{DefaultEditor, Result};
use std::collections::VecDeque;
use std::fmt;
use std::mem;
use std::str;
use uuid::Uuid;
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
                block_groups_rest_bytes.1.as_ptr() as *const u8,
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

    pub fn read_dir_inode(&self, inode: usize) -> std::io::Result<Vec<(usize, &NulStr)>> {
        let mut ret = Vec::new();
        let root = self.get_inode(inode);
        if root.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "inode is not a directory",
            ));
        }
        // println!("in read_dir_inode, #{} : {:?}", inode, root);
        // println!(
        //     "following direct pointer to data block: {}",
        //     root.direct_pointer[0]
        // );
        let entry_ptr = self.blocks[root.direct_pointer[0] as usize - self.block_offset].as_ptr();
        let mut byte_offset: isize = 0;
        while byte_offset < root.size_low as isize {
            // <- todo, support large directories
            let directory = unsafe { &*(entry_ptr.offset(byte_offset) as *const DirectoryEntry) };
            // println!("{:?}", directory);
            byte_offset += directory.entry_size as isize;
            ret.push((directory.inode as usize, &directory.name));
        }
        Ok(ret)
    }

    // lifetime of the return value needs to be the same as the lifetime of path
    pub fn follow_path_tuple<'a, 'b>(
        self: &'a Ext2,
        path: &'b str,
        dirs: Vec<(usize, &NulStr)>,
    ) -> (usize, &'b str) {
        let mut candidate_directories: VecDeque<&str> = path.split('/').collect();
        let mut dirs: Vec<(usize, &NulStr)> = dirs;
        let mut possible_inode: usize = 2;
        // directory where the call is made from
        let initial_dir = dirs[0].0;
        // canddiate is a borrow from the scope of this function, borrowing something that lives for the scope of this fucntion
        // so when we reference at the end of the function the reference will die
        // so we can't return the reference bc it is to something that lives on the stack which will die
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
            } else {
                let inode = self.get_inode(possible_inode);
                // check type permission of inode, for last inode can be not a directory (for cat)
                if inode.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY
                    && candidate_directories.len() != 0
                {
                    println!("not a directory: {}", candidate.unwrap());
                    // return (initial_dir, candidate);
                    // lifetime is how long is the scope of the thing passed in
                    return (initial_dir, candidate.unwrap());
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
        // return (possible_inode, &candidate);
        return (possible_inode, candidate.unwrap());
    }

    // lifetime of the return value needs to be the same as the lifetime of path
    pub fn follow_path(&self, path: &str, dirs: Vec<(usize, &NulStr)>) -> usize {
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
            } else {
                let inode = self.get_inode(possible_inode);
                // check type permission of inode, for last inode can be not a directory (for cat)
                if inode.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY
                    && candidate_directories.len() != 0
                {
                    println!("not a directory: {}", candidate.unwrap());
                    return initial_dir;
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
        return possible_inode;
    }

    pub fn read_file_inode(&self, inode: usize) -> std::io::Result<&[u8]> {
        // let mut ret = Vec::new();
        let root = self.get_inode(inode);
        // make sure we are reading a file
        if root.type_perm & TypePerm::FILE != TypePerm::FILE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "inode is not a file",
            ));
        }
        // println!("in read_dir_inode, #{} : {:?}", inode, root);
        // println!(
        //     "following direct pointer to data block: {}",
        //     root.direct_pointer[0]
        // );
        let file_bytes = self.blocks[root.direct_pointer[0] as usize - self.block_offset];
        // println!("{}", str::from_utf8(file_bytes).unwrap());
        // let entry_ptr = self.blocks[root.direct_pointer[0] as usize - self.block_offset].as_ptr();
        // let mut byte_offset: isize = 0;
        Ok(file_bytes)
    }

    pub fn ls(&self, dirs: Vec<(usize, &NulStr)>, command: String) -> std::io::Result<()> {
        let elts: Vec<&str> = command.split(' ').collect();
        if elts.len() == 1 {
            for dir in &dirs {
                print!("{}\t", dir.1);
            }
            println!();
        } else {
            let paths = elts[1];
            let inode = self.follow_path(paths, dirs);
            let possible_inode = self.get_inode(inode);
            if possible_inode.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
                println!("not a directory: {}", paths);
            }
            // get directories for
            let dirs_to_show: Option<Vec<(usize, &NulStr)>> = match self.read_dir_inode(inode) {
                Ok(dir_listing) => Some(dir_listing),
                Err(_) => None,
            };
            if dirs_to_show.is_none() {
                println!("unable to read directory in ls");
            }
            for dir in &dirs_to_show.unwrap() {
                print!("{}\t", dir.1);
            }
            println!();
        }
        return Ok(());
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
    let disk = include_bytes!("../myfs.ext2");
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
                ext2.ls(dirs, line);
            } else if line.starts_with("cd") {
                // `cd` with no arguments, cd goes back to root
                // `cd dir_name` moves cwd to that directory
                let elts: Vec<&str> = line.split(' ').collect();
                if elts.len() == 1 {
                    current_working_inode = 2;
                } else {
                    let paths = elts[1];
                    let inode = ext2.follow_path(paths, dirs);
                    let possible_inode = ext2.get_inode(inode);
                    // let possible_inode = ext2.get_inode(inode);
                    if possible_inode.type_perm & TypePerm::DIRECTORY != TypePerm::DIRECTORY {
                        println!("not a directory: {}", paths);
                    }
                    current_working_inode = inode;
                }
            } else if line.starts_with("mkdir") {
                // `mkdir childname`
                // create a directory with the given name, add a link to cwd
                // consider supporting `-p path/to_file` to create a path of directories
                println!("mkdir not yet implemented");
            } else if line.starts_with("cat") {
                // `cat filename`
                // print the contents of filename to stdout
                // if it's a directory, print a nice error
                let elts: Vec<&str> = line.split(' ').collect();
                if elts.len() == 1 {
                    print!("must pass file to show");
                } else {
                    let paths = elts[1];
                    // get inode of potential file
                    let possible_inode = ext2.follow_path(paths, dirs);
                    let inode = ext2.get_inode(possible_inode);
                    if inode.type_perm & TypePerm::FILE != TypePerm::FILE {
                        println!("not a file: {}", paths);
                    } else {
                        let s = match ext2.read_file_inode(possible_inode) {
                            Ok(file_data) => file_data,
                            Err(_) => {
                                println!("unable to read directory in ls");
                                break;
                            }
                        };
                        println!("{}", str::from_utf8(s).unwrap());
                    }
                }
                // println!("cat not yet implemented");
            } else if line.starts_with("rm") {
                // `rm target`
                // unlink a file or empty directory
                println!("rm not yet implemented");
            } else if line.starts_with("mount") {
                // `mount host_filename mountpoint`
                // mount an ext2 filesystem over an existing empty directory
                println!("mount not yet implemented");
            } else if line.starts_with("link") {
                // `link arg_1 arg_2`
                // create a hard link from arg_1 to arg_2
                // consider what to do if arg2 does- or does-not end in "/"
                // and/or if arg2 is an existing directory name
                println!("link not yet implemented");
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
