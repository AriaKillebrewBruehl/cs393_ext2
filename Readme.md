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

### `follow-path` function

- parsing the path that got passed in and making sure all the elements were directories
  - the last element doesn't need to be a directory so this works for cat as well
- update current working inode to be last element of the path

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

Credits: Reed College CS393 students, @tzlil on the Rust #osdev discord
