# Aria and Caden CSCI 393 Final Project

## Summary

This is our project which builds on an Implementation of Ext2 in Rust. We started with the code given to the class and set out to expand and improve the functionality. This is a summary of what we did and the process.

We ended up cleaning up the code to have functions for all the input commands. We also implemented a follow path function so that when you `ls` or `cd`, you can provide a file path instead of being limited to only files in the current directory. We also implemented cat and a limited version of mkdir. These improvements are detailed in the Design and Implementation section below.

Here's an example session:

```
:> ls
.      ..      lost+found      test_directory  hello.txt
:> cd test_directory
:> ls
.      ..      file_in_folder.txt
:> cd ..
:> cat test_directory/file_in_folder.txt
Hello! I'm a file inside a folder.
:> ls
PU      ..      lost+found      test_directory
:> mkdir hello
:> ls
.      ..      lost+found      test_directory  hello.txt       hello
:>
```

## Design and Implementation

### Code cleanup

We decided that it would be a lot easier to read if all the command line calls were handled by their own function. This allowed us to work on different parts of the program at once and to better diagnose where the trouble spots were in our code.

### `follow-path` function

In the original implementation of `cd` it was not possible to follow folder paths. You could not, for example, do `cd dirA/dirB/dirC`. To support this functionality we wrote a `follow_path` method for the `ext2` struct which takes a path name and a vector of the directories of the current working inode as input and returns the inode number for the final directory, if it exists.

The implementation was relatively straight forward:

- save the directory that the call is made from, `initial_dir`, which will be returned if the path cannot be followed

- split the path into a vector of the directories that must be followed, `candidate_directories`

- set `dirs` to be a vector of all `DirectoryEntries` currently reachable

- for each `candidate` in `candidate_directories`:

  - make sure `candidate` exists in `dirs`
    - if it does not return `initial_dir`
  - get inode number form `candidate`
  - make sure `candidate` is a has `TypePerm::DIRECTORY` set (this does not need to be true for the last `candidate`)
    - if it does not return `initial_dir`
  - update `dirs` to be `DirectoryEntries` of `candidate`

- return the inode number of the final `candidate`

The `follow_path` function also allows the user to run the `cat` command with a file path (`cat dirA/dirB/dirC/file.txt`).

## `cat`

This part was fairly simple, as in the original code, calling `ls` on a file nearly printed out its contents. The concept is the same: we want to read the pointers in the inode for a file, except in this case the pointers point to data blocks full of characters rather than Directory Entries. So we can go through the blocks that direct pointers point to, cast them as Nullstrings, and print the result. This would work for files that are longer than can fit in direct pointers, except you would have to parse the doubly and triply indirect pointer as well. This is left as an exercise for the reader.

I'm kidding. But in all seriousness, we understood how this works and could have implemented it, but we were more interested in the `mkdir` problem. As two seniors with limited time, we chose the more interesting problem.

## `mkdir`

`mkdir` was much more difficult than expected. When we initially approached this problem we figured the steps for creating a new directory would be straight forward:

1. make a new `DirectoryEntry` object with the correct `name` and `entry_size`
2. find the location in memory where this `DirectoryEntry` will be inserted
3. insert it

All three of these steps proved quite difficult to complete.

To complete step `2` we knew we needed to find the end of the `DirectoryEntry`s for our current inode. We thought this could be done using the `size_low` and `size_high` attributes of the `Inode` object and casting the bytes of the direct block pointers as `DirectoryEntry`s until we reached the last one. This was not a viable approach since a `DirectoryEntry` can span multiple blocks with the last entry being padded to fill the whole block:

```
direct block 0:
|| DE0            | DE1       | DE2    ||

direct block 1:
|| DE2 cont.| DE3                      ||
```

Try to cast `direct block 1` as `DirectoryEntry` objects directly is impossible since this block begins with a partial entry.

To resolve this issue we needed to read the data from the direct blocks into a contiguous array, `contiguous_data`. This was done by reading the data out of the blocks and into `contiguous_data` as bytes. This solved the issue of entries that spanned multiple blocks but

- given a directory inode

  - find the end of its directory entries
  - add a new directory entry at the end of this

  - initially to do this we tried to do this by casting bytes of data from the direct blocks as directory entries as the original code is

    - this doesn't work because DEs are not all the same size and some of them can span multiple blocks

    - so then we needed to read all the data out of the direct blocks as bytes into a vector called `contiguous_data` to fix the issue of DEs spanning blocks

    - then we thought we'd just create a DE object for our new entry and add it to the end of `contig_dat` and write it back out

      - this didn't work bc you can't make a DE object because the name field is a `NulStr` and is not sized

        - so to work around this we needed to manually add all the elements of the entry to the `contig_data` vect

          - we also had to deal w the fact that the last DE gets padded to fill out a block

            - so we needed to change the entry size of the last DE before adding in our new one

        - then we tried to write the `contig_data` vect back out but we realized we couldn't since the FS is part of the binary / is loaded in at compile time

          - so then we needed to read the FS in at run time so we could actually write to the direct blocks

### What We Would Do Differently

There is so much to say here and so little time to say it.

- start more from scratch

We think we also would have been more diligent in writing tests. This is really still related to stripping out the code, because it its current state tests would be extremely hard to write.

## Areas of Expansion

Here are areas where the code could be improved, specifically the functionality

- Reading data blocks
  - Right now, both `cat` and `mkdir` only read the direct pointers of the inode.
- `mkdir` improvements
  - the new entry created by mkdir needs to be inflated to fill out the end of the data block
  - the new entry also needs to actually be created: it needs an inode with entries for `.` and `..`
- Writing out
  - We are mounting the `myfs.ext2` file rather than reading it into the binary at compile time, but we are not writing out the changes made by `mkdir`. Once the previous issues are fixed, one should write to the `ext2` file so that the changes persist to the next program run.

Credits: Reed College CS393 students, @tzlil on the Rust #osdev discord
