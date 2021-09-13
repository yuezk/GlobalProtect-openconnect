include(cmake/CPM.cmake)

function(add_3rdparty)
    cmake_parse_arguments(add_3rdparty_args "EXCLUDE_FROM_ALL" "NAME" "" ${ARGN})
    set(NAME ${add_3rdparty_args_NAME})

    if(EXISTS "${CMAKE_SOURCE_DIR}/3rdparty/${NAME}/CMakeLists.txt")
        message(STATUS "Found third party locally for ${NAME}")

        if(${add_3rdparty_args_EXCLUDE_FROM_ALL})
            set(addSubdirectoryExtraArgs EXCLUDE_FROM_ALL)
        else()
            set(addSubdirectoryExtraArgs "")
        endif()

        add_subdirectory(
            "${CMAKE_SOURCE_DIR}/3rdparty/${NAME}"
            ${addSubdirectoryExtraArgs}
        )
        return()
    endif()
    message(STATUS "Using CPM to download ${NAME}") 
    CPMAddPackage(${ARGN})
endfunction()
