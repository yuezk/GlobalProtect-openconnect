cmake_minimum_required(VERSION 3.10.0)

set(CMAKE_CXX_STANDARD 17)
project(inih)

add_library(inih STATIC
    ini.h
    ini.c
    cpp/INIReader.h
    cpp/INIReader.cpp
)
target_include_directories(inih PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/cpp")
