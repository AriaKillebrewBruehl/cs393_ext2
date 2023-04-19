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

1. find the location in memory where this `DirectoryEntry` will be inserted
2. make a new `DirectoryEntry` object with the correct `name` and `entry_size`
3. insert it

All three of these steps proved quite difficult to complete.

To complete step `1` we knew we needed to find the end of the `DirectoryEntry`s for our current inode. We thought this could be done using the `size_low` and `size_high` attributes of the `Inode` object and casting the bytes of the direct block pointers as `DirectoryEntry`s until we reached the last one. This was not a viable approach since a `DirectoryEntry` can span multiple blocks with the last entry being padded to fill the whole block:

```
direct block 0:
|| DE0            | DE1       | DE2    ||

direct block 1:
|| DE2 cont.| DE3                      ||
```

Try to cast `direct block 1` as `DirectoryEntry` objects directly is impossible since this block begins with a partial entry.

To resolve this issue we needed to read the data from the direct blocks into a contiguous array, `contiguous_data`. This was done by reading the data out of the blocks and into `contiguous_data` as bytes. This solved the issue of entries that spanned multiple blocks. The `contiguous_data` vector would still be padded with `0` bytes since the last `DirectoryEntry` is extended to fill the entire block. To fix this we simply trim `contiguous_data` to remove any `0`s at the end of the vector.

The next step was creating the new directory entry and adding it to the end of `contiguous_data`. Initially we attempted to do this by creating a new `DirectoryEntry` object with the correct values and simply appending those bytes to the end of `contiguous_data`. This was impossible since a `DirectoryEntry` object contains a `NulStr` which is not sized. To append the entry to `contiguous_data` we needed to manually append the bytes for each of the fields of the entry to the vector.

Then, to update the actual data store in the direct blocks, we wrote the bytes stored in `contiguous_data` back out to the data blocks. This was impossible with the original code since the file system was being loaded in at compile time and was therefore encoded in the binary. So to be able to actually write to the data blocks we needed to change the loading in of the file system to be done at runtime. Once we got this working we wrote `contiguous_data` back out to the data blocks.

Sadly we were still not done. Since the `ext2` file system pads the last directory entry to fill up the rest of the data block the last entry (second to last after adding the new entry) will have a very large entry size. This meant that when trying to read the directory entires back out (i.e. `mkdir test; ls`) we would not see our new entry (`test`). To resolve this issue we needed to update the size of the second to last entry to be the actual size of the entry. Once we did this we had `mkdir` working! Kinda...

### What We Would Do Differently

There is so much to say here and so little time to say it. We think in an ideal world where our time was less limited, we would have stripped out a lot of the code and started from scratch. The C memory tricks are useful, but the code is very fragile and we found it resistant to change. We probably would have been better of keeping the structs that were there, changing how the filesystem is read so that it is not loaded into the binary, and staying away from using `unsafe` as much as possible. The great thing about Rust is that if we go it to compile, it usually worked. The bad thing is that it rarely did compile the first time when changes were made to the code's structure.

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
