# This is just a simple CMake file to compile a test that loads a .fst file
# using the GtkWave source for it, for testing purposes.
#
# The fst source is copied from the gtkwave4 directory of commit 7b972229b3b674a97702ff63064f714e69b53bb9
# (23rd November 2022). I also ran clang-format over it, and added some debugging stuff.

# Easiest to use on Linux of via WSL since ZLib is a pain and you can't use miniz.
#
# sudo apt install cmake zlib1g-dev
# mkdir build_linux
# cd build_linux
# cmake -DZLIB_LIBRARY=/usr/lib/x86_64-linux-gnu/libz.so ..

cmake_minimum_required(VERSION 3.16.0)
project(fst)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED TRUE)
set(CMAKE_EXPORT_COMPILE_COMMANDS TRUE)

# apt install zlib1g-dev
include(FindZLIB)

if (NOT ZLIB_FOUND)
    message(FATAL "ZLib not found")
endif()

add_executable(fst
    "fst/fastlz.c"
    "fst/fastlz.h"
    "fst/fst_win_unistd.h"
    "fst/fstapi.c"
    "fst/fstapi.h"
    "fst/lz4.c"
    "fst/lz4.h"
    "main.cpp"
    "config.h"
)

target_include_directories(fst PRIVATE
    "."
    "fst"
)

target_link_libraries(fst PRIVATE ZLIB::ZLIB)
