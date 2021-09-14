include(ExternalProject)

function(add_3rdparty NAME)
    set(oneValueArgs GIT_REPOSITORY GIT_TAG)
    cmake_parse_arguments(add_3rdparty_args "${oneValueArgs}" "" ${ARGN})

    if(EXISTS "${CMAKE_SOURCE_DIR}/3rdparty/${NAME}/CMakeLists.txt")
        message(STATUS "Found third party locally for ${NAME}")

        ExternalProject_Add(
            ${NAME}-${PROJECT_NAME}
            PREFIX ${CMAKE_CURRENT_BINARY_DIR}/${NAME}
            SOURCE_DIR "${CMAKE_SOURCE_DIR}/3rdparty/${NAME}"
            INSTALL_COMMAND ""
            "${add_3rdparty_args_UNPARSED_ARGUMENTS}"
        )
        return()
    endif()

    message(STATUS "Using ExternalProject to download ${NAME}") 
    ExternalProject_Add(
        ${NAME}-${PROJECT_NAME}
        PREFIX ${CMAKE_CURRENT_BINARY_DIR}/${NAME}
        INSTALL_COMMAND ""
        "${ARGN}"
    )
endfunction()
