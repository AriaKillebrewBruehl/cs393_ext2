# Aria and Caden CSCI 393 Final Project

This is a starting point for parsing and navigating ext2 file systems.
`cargo run` will start a session that looks like a shell. All you can
do for now are the `ls`, and `cd` commands.
It's left as an exercise to implement `cat` to view the contents of files,
and removing other limitations.

Here's an example session:

** REDO THIS**

```
% cargo run
   <building and intro stuff>
:> ls
.	..	lost+found	test_directory	hello.txt
:> cat hello.txt
cat not yet implemented
:> cd test_directory
:> ls
.	..	file_in_folder.txt
:> cd file_in_folder.txt    # <- whoops
:> ls
'm a file inside a folder.  # <- whoops^2

:>
```

Credits: Reed College CS393 students, @tzlil on the Rust #osdev discord

## Aria and Caden Project To-Dos:

What we actually did

- [x] make bigger files
- [x] clean up code to have functions for all the input commands
- follow path
- cat
- [x] `mkdir`

## Design and Implementation

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

- read all the bytes in the direct block pointers
  - not working for large files

## `mkdir`

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

- start more from scratch

  - spent way too much time trying to understand someone else's code

- less unsafe

## Areas of Expansion

- actually making it so you can `cd` into a new directory

  - allocating an inode for the new directory
  - giving that inode DEs for `.` and `..`

- reading indirect, doubly, triply indirect pointers for large files and directories

- saving state of FS

-
