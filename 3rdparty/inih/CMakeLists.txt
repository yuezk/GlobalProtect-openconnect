cmake_minimum_required(VERSION 3.10.0)

project(inih)

add_library(inih STATIC
    ini.h
    ini.c
    cpp/INIReader.h
    cpp/INIReader.cpp
)
target_include_directories(inih INTERFACE ${CMAKE_CURRENT_SOURCE_DIR})