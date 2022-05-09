# - Try to find NetworkManager
# Once done this will define
#
#  NETWORKMANAGER_FOUND - system has NetworkManager
#  NETWORKMANAGER_INCLUDE_DIRS - the NetworkManager include directories
#  NETWORKMANAGER_LIBRARIES - the libraries needed to use NetworkManager
#  NETWORKMANAGER_CFLAGS - Compiler switches required for using NetworkManager
#  NETWORKMANAGER_VERSION - version number of NetworkManager

# Copyright (c) 2006, Alexander Neundorf, <neundorf@kde.org>
# Copyright (c) 2007, Will Stephenson, <wstephenson@kde.org>
# Copyright (c) 2015-2018, Jan Grulich, <jgrulich@redhat.com>

# Redistribution and use in source and binary forms, with or without
# modification, are permitted provided that the following conditions
# are met:
# 1. Redistributions of source code must retain the above copyright
#    notice, this list of conditions and the following disclaimer.
# 2. Redistributions in binary form must reproduce the above copyright
#    notice, this list of conditions and the following disclaimer in the
#    documentation and/or other materials provided with the distribution.
# 3. Neither the name of the University nor the names of its contributors
#    may be used to endorse or promote products derived from this software
#    without specific prior written permission.
#
# THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
# ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
# IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
# ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
# FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
# DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
# OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
# HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
# LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
# OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
# SUCH DAMAGE.

IF (NETWORKMANAGER_INCLUDE_DIRS)
    # in cache already
    SET(NetworkManager_FIND_QUIETLY TRUE)
ENDIF (NETWORKMANAGER_INCLUDE_DIRS)

IF (NOT WIN32)
    find_package(PkgConfig)
    PKG_SEARCH_MODULE(NETWORKMANAGER libnm)
    IF (NETWORKMANAGER_FOUND)
        IF (NetworkManager_FIND_VERSION AND ("${NETWORKMANAGER_VERSION}" VERSION_LESS "${NetworkManager_FIND_VERSION}"))
            MESSAGE(FATAL_ERROR "NetworkManager ${NETWORKMANAGER_VERSION} is too old, need at least ${NetworkManager_FIND_VERSION}")
        ELSE ()
            IF (NOT NetworkManager_FIND_QUIETLY)
                MESSAGE(STATUS "Found NetworkManager: ${NETWORKMANAGER_LIBRARY_DIRS}")
            ENDIF ()
        ENDIF ()
    ELSE ()
        MESSAGE(FATAL_ERROR "Could NOT find NetworkManager, check FindPkgConfig output above!")
    ENDIF ()
ENDIF (NOT WIN32)

MARK_AS_ADVANCED(NETWORKMANAGER_INCLUDE_DIRS)
