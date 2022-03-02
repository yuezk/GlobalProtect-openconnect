#!/bin/bash
#
# █▄▄ █░█ █▀▄▀█ █▀█ ▄▄ █░█ █▀▀ █▀█ █▀ █ █▀█ █▄░█
# █▄█ █▄█ █░▀░█ █▀▀ ░░ ▀▄▀ ██▄ █▀▄ ▄█ █ █▄█ █░▀█
#
# Description:
#   - This script automates bumping the git software project's version using automation.

#   - It does several things that are typically required for releasing a Git repository, like git tagging, 
#     automatic updating of CHANGELOG.md, and incrementing the version number in various JSON files.

#     - Increments / suggests the current software project's version number
#     - Adds a Git tag, named after the chosen version number
#     - Updates CHANGELOG.md
#     - Updates VERSION file
#     - Commits files to a new branch  
#     - Pushes to remote (optionally)
#     - Updates "version" : "x.x.x" tag in JSON files if [-v file1 -v file2...] argument is supplied.
#
# Usage: 
#   ./bump-version.sh [-v <version number>] [-m <release message>] [-j <file1>] [-j <file2>].. [-n] [-p] [-b] [-h]
#
# Options:
#   -v <version number>	  Specify a manual version number
#   -m <release message>	Custom release message.
#   -f <filename.json>	  Update version number inside JSON files.
# 			                  * For multiple files, add a separate -f option for each one,
#	  		                  * For example: ./bump-version.sh -f src/plugin/package.json -f composer.json
#   -p <repository alias> Push commits to remote repository, eg `-p origin`
#   -n 	                  Don't perform a commit automatically.
#	  		                  * You may want to do that yourself, for example.
#   -b                    Don't create automatic `release-<version>` branch
#   -h 	                  Show help message.

#
# Detailed notes:
#   – The contents of the `VERSION` file which should be a semantic version number such as "1.2.3" 
#     or even "1.2.3-beta+001.ab"
#   
#   – It pulls a list of changes from git history & prepends to a file called CHANGELOG.md 
#     under the title of the new version # number, allows the user to review and update the changelist
#
#   – Creates a Git tag with the version number
#
#   - Creates automatic `release-<version>` branch
#
#   – Commits the new version to the current repository
#
#   – Optionally pushes the commit to remote repository
#
#   – Make sure to set execute permissions for the script, eg `$ chmod 755 bump-version.sh`
#
# Credits:
#   – https://github.com/jv-k/bump-version
#
#   - Inspired by the scripts from @pete-otaqui and @mareksuscak
#     https://gist.github.com/pete-otaqui/4188238
#     https://gist.github.com/mareksuscak/1f206fbc3bb9d97dec9c
#

NOW="$(date +'%B %d, %Y')"

# ANSI/VT100 colours
YELLOW='\033[1;33m'
LIGHTYELLOW='\033[0;33m'
RED='\033[0;31m'
LIGHTRED='\033[1;31m'
GREEN='\033[0;32m'
LIGHTGREEN='\033[1;32m'
BLUE='\033[0;34m'
LIGHTBLUE='\033[1;34m'
PURPLE='\033[0;35m'
LIGHTPURPLE='\033[1;35m'
CYAN='\033[0;36m'
LIGHTCYAN='\033[1;36m'
WHITE='\033[1;37m'
LIGHTGRAY='\033[0;37m'
DARKGRAY='\033[1;30m'
BOLD="\033[1m"
INVERT="\033[7m"
RESET='\033[0m'

# Default options
FLAG_JSON="false"
FLAG_PUSH="false"

I_OK="✅"; I_STOP="🚫"; I_ERROR="❌"; I_END="👋🏻"

S_NORM="${WHITE}"
S_LIGHT="${LIGHTGRAY}"
S_NOTICE="${GREEN}"
S_QUESTION="${YELLOW}"
S_WARN="${LIGHTRED}"
S_ERROR="${RED}"

V_SUGGEST="0.1.0" # This is suggested in case VERSION file or user supplied version via -v is missing
GIT_MSG=""
REL_NOTE=""
REL_PREFIX="release-"
PUSH_DEST="origin"

# Show credits & help
usage() { 
  echo -e "$GREEN"\
          "\n █▄▄ █░█ █▀▄▀█ █▀█ ▄▄ █░█ █▀▀ █▀█ █▀ █ █▀█ █▄░█  "\
          "\n █▄█ █▄█ █░▀░█ █▀▀ ░░ ▀▄▀ ██▄ █▀▄ ▄█ █ █▄█ █░▀█  "\
          "\n\t\t\t\t\t$LIGHTGRAY v${SCRIPT_VER}"\

  echo -e " ${S_NORM}${BOLD}Usage:${RESET}"\
          "\n $0 [-v <version number>] [-m <release message>] [-j <file1>] [-j <file2>].. [-n] [-p] [-h]" 1>&2; 
  
  echo -e "\n ${S_NORM}${BOLD}Options:${RESET}"
  echo -e " $S_WARN-v$S_NORM <version number>\tSpecify a manual version number"
  echo -e " $S_WARN-m$S_NORM <release message>\tCustom release message."
  echo -e " $S_WARN-f$S_NORM <filename.json>\tUpdate version number inside JSON files."\
          "\n\t\t\t* For multiple files, add a separate -f option for each one,"\
          "\n\t\t\t* For example: ./bump-version.sh -f src/plugin/package.json -f composer.json"
  echo -e " $S_WARN-p$S_NORM \t\t\tPush commits to ORIGIN. "
  echo -e " $S_WARN-n$S_NORM \t\t\tDon't perform a commit automatically. "\
          "\n\t\t\t* You may want to do that manually after checking everything, for example."
  echo -e " $S_WARN-b$S_NORM \t\t\tDon't create automatic \`release-<version>\` branch"
  echo -e " $S_WARN-h$S_NORM \t\t\tShow this help message. "
  echo -e "\n ${S_NORM}${BOLD}Author:$S_LIGHT https://github.com/jv-t/bump-version $RESET\n"

}

# If there are no commits in repo, quit, because you can't tag with zero commits.
check-commits-exist() {
  git rev-parse HEAD &> /dev/null
  if [ ! "$?" -eq 0 ]; then
    echo -e "\n${I_STOP} ${S_ERROR}Your current branch doesn't have any commits yet. Can't tag without at least one commit." >&2
    echo    
    exit 1
  fi
}

get-commit-msg() {
 echo Bumped $([ -n "${V_PREV}" ] && echo "${V_PREV} –>" || echo "to ") "$V_USR_INPUT"
}

exit_abnormal() {
  echo -e " ${S_LIGHT}––––––"
  usage # Show help
  exit 1
}

# Process script options
process-arguments() {
  local OPTIONS OPTIND OPTARG
    
  # Get positional parameters
  JSON_FILES=( )
  while getopts ":v:p:m:f:hbn" OPTIONS; do # Note: Adding the first : before the flags takes control of flags and prevents default error msgs.
    case "$OPTIONS" in
      h )
        # Show help
        exit_abnormal
      ;;
      v )
        # User has supplied a version number
        V_USR_SUPPLIED=$OPTARG
      ;;
      m )
        REL_NOTE=$OPTARG
        # Custom release note
        echo -e "\n${S_LIGHT}Option set: ${S_NOTICE}Release note:" ${S_NORM}"'"$REL_NOTE"'"
      ;;
      f )
        FLAG_JSON=true
        echo -e "\n${S_LIGHT}Option set: ${S_NOTICE}JSON file via [-f]: <${S_NORM}${OPTARG}${S_LIGHT}>"
        # Store JSON filenames(s)
        JSON_FILES+=($OPTARG)
      ;;
      p )
        FLAG_PUSH=true
        PUSH_DEST=${OPTARG} # Replace default with user input
        echo -e "\n${S_LIGHT}Option set: ${S_NOTICE}Pushing to <${S_NORM}${PUSH_DEST}${S_LIGHT}>, as the last action in this script."
      ;;
      n )
        FLAG_NOCOMMIT=true
        echo -e "\n${S_LIGHT}Option set: ${S_NOTICE}Disable commit after tagging."
      ;;
      b )
        FLAG_NOBRANCH=true
        echo -e "\n${S_LIGHT}Option set: ${S_NOTICE}Disable committing to new branch."
      ;;
      \? )
        echo -e "\n${I_ERROR}${S_ERROR} Invalid option: ${S_WARN}-$OPTARG" >&2
        echo
        exit_abnormal
      ;;        
      : )
        echo -e "\n${I_ERROR}${S_ERROR} Option ${S_WARN}-$OPTARG ${S_ERROR}requires an argument." >&2
        echo
        exit_abnormal
      ;;      
    esac
  done
}

# Suggests version from VERSION file, or grabs from user supplied -v <version>.
# If none is set, suggest default from options.
process-version() {
  if [ -f VERSION ] && [ -s VERSION ]; then    
    V_PREV=`cat VERSION`

    echo -e "\n${S_NOTICE}Current version from <${S_NORM}VERSION${S_NOTICE}> file: ${S_NORM}$V_PREV"

    # Suggest incremented value from VERSION file
    V_PREV_LIST=(`echo $V_PREV | tr '.' ' '`)
    V_MAJOR=${V_PREV_LIST[0]}; V_MINOR=${V_PREV_LIST[1]}; V_PATCH=${V_PREV_LIST[2]};
    
    # Test if V_PATCH is a number, then increment it. Otherwise, do nothing
    if [ "$V_PATCH" -eq "$V_PATCH" ] 2>/dev/null; then # discard stderr (2) output to black hole (suppress it)
      V_PATCH=$((V_PATCH + 1)) # Increment
    fi

    V_SUGGEST="$V_MAJOR.$V_MINOR.$V_PATCH"
  else
    echo -ne "\n${S_WARN}The [${S_NORM}VERSION${S_WARN}] "
    if [ ! -f VERSION ]; then 
      echo "file was not found.";     
    elif [ ! -s VERSION ]; then 
      echo "file is empty."; 
    fi
  fi

  # If a version number is supplied by the user with [-v <version number>], then use it 
  if [ -n "$V_USR_SUPPLIED" ]; then
    echo -e "\n${S_NOTICE}You selected version using [-v]:" "${S_WARN}${V_USR_SUPPLIED}"      
    V_USR_INPUT="${V_USR_SUPPLIED}"
  else
    echo -ne "\n${S_QUESTION}Enter a new version number [${S_NORM}$V_SUGGEST${S_QUESTION}]: "
    echo -ne "$S_WARN"
    read V_USR_INPUT

    if [ "$V_USR_INPUT" = "" ]; then
      V_USR_INPUT="${V_SUGGEST}"
    fi
  fi
  
  # echo -e "${S_NOTICE}Setting version to [${S_NORM}${V_USR_INPUT}${S_NOTICE}] ...."
}

# Only tag if tag doesn't already exist
check-tag-exists() {
  TAG_CHECK_EXISTS=`git tag -l v"$V_USR_INPUT"`
  if [ -n "$TAG_CHECK_EXISTS" ]; then
    echo -e "\n${I_STOP} ${S_ERROR}Error: A release with that tag version number already exists!\n"
    exit 0
  fi
}

# $1 : version
# $2 : release note
tag() {
  if [ -z "$2" ]; then
    # Default release note
    git tag -a "v$1" -m "Tag version $1."
  else
    # Custom release note
    git tag -a "v$1" -m "$2"
  fi
  echo -e "\n${I_OK} ${S_NOTICE}Added GIT tag"
}

# Change `version:` value in JSON files, like packager.json, composer.json, etc
bump-json-files() {
  if [ "$FLAG_JSON" != true ]; then return; fi
  
  JSON_PROCESSED=( ) # holds filenames after they've been changed

  for FILE in "${JSON_FILES[@]}"; do
    if [ -f $FILE ]; then
      # Get the existing version number
      V_OLD=$( sed -n 's/.*"version": "\(.*\)",/\1/p' $FILE )

      if [ "$V_OLD" = "$V_USR_INPUT" ]; then
        echo -e "\n${S_WARN}File <${S_NORM}$FILE${S_WARN}> already contains version: ${S_NORM}$V_OLD"
      else
        # Write to output file
        FILE_MSG=`sed -i .temp "s/\"version\": \"$V_OLD\"/\"version\": \"$V_USR_INPUT\"/g" $FILE 2>&1`
        if [ "$?" -eq 0 ]; then
          echo -e "\n${I_OK} ${S_NOTICE}Updated file: <${S_NOTICE}$FILE${S_LIGHT}> from ${S_NORM}$V_OLD -> $V_USR_INPUT"
          rm -f ${FILE}.temp          
          # Add file change to commit message:
          GIT_MSG+="${GIT_MSG}Updated $FILE, "
        else
          echo -e "\n${I_STOP} ${S_ERROR}Error\n$PUSH_MSG\n"
        fi
      fi

      JSON_PROCESSED+=($FILE)
    else
      echo -e "\n${S_WARN}File <${S_NORM}$FILE${S_WARN}> not found."
    fi
  done
  # Stage files that were changed:
  [ -n "${JSON_PROCESSED}" ] && git add "${JSON_PROCESSED[@]}"
}

# Handle VERSION file
do-versionfile() {
  [ -f VERSION ] && ACTION_MSG="Updated" || ACTION_MSG="Created"

  GIT_MSG+="${ACTION_MSG} VERSION, "
  echo $V_USR_INPUT | tr -d "\n" > VERSION # Create file
  echo -e "\n${I_OK} ${S_NOTICE}${ACTION_MSG} [${S_NORM}VERSION${S_NOTICE}] file"

  # Stage file for commit
  git add VERSION
}

# Dump git log history to CHANGELOG.md
do-changelog() {  

  # Log latest commits to CHANGELOG.md:
  # Get latest commits
  LOG_MSG=`git log --pretty=format:"- %s" $([ -n "$V_PREV" ] && echo "v${V_PREV}...HEAD") 2>&1`
  if [ ! "$?" -eq 0 ]; then
    echo -e "\n${I_STOP} ${S_ERROR}Error getting commit history for logging to CHANGELOG.\n$LOG_MSG\n"
    exit 1
  fi
  
  [ -f CHANGELOG.md ] && ACTION_MSG="Updated" || ACTION_MSG="Created"
  # Add info to commit message for later:
  GIT_MSG+="${ACTION_MSG} CHANGELOG.md, "
 
  # Add heading
  echo "## $V_USR_INPUT ($NOW)" > tmpfile

  # Log the bumping commit:
  # - The final commit is done after do-changelog(), so we need to create the log entry for it manually:
  echo "- ${GIT_MSG}$(get-commit-msg)" >> tmpfile
  # Add previous commits
  [ -n "$LOG_MSG" ] && echo "$LOG_MSG" >> tmpfile
  
  echo -en "\n" >> tmpfile

  if [ -f CHANGELOG.md ]; then
    # Append existing log
    cat CHANGELOG.md >> tmpfile
  else
    echo -e "\n${S_WARN}A [${S_NORM}CHANGELOG.md${S_WARN}] file was not found."  
  fi

  mv tmpfile CHANGELOG.md
  
  # User prompts
  echo -e "\n${I_OK} ${S_NOTICE}${ACTION_MSG} [${S_NORM}CHANGELOG.md${S_NOTICE}] file"
  # Pause & allow user to open and edit the file:
  echo -en "\n${S_QUESTION}Make adjustments to [${S_NORM}CHANGELOG.md${S_QUESTION}] if required now. Press <enter> to continue."
  read

  # Stage log file, to commit later
  git add CHANGELOG.md
}

#
check-branch-exist() {
  [ "$FLAG_NOBRANCH" = true ] && return

  BRANCH_MSG=`git rev-parse --verify "${REL_PREFIX}${V_USR_INPUT}" 2>&1`
  if [ "$?" -eq 0 ]; then
    echo -e "\n${I_STOP} ${S_ERROR}Error: Branch <${S_NORM}${REL_PREFIX}${V_USR_INPUT}${S_ERROR}> already exists!\n"
    exit 1
  fi  
}

# 
do-branch() {
  [ "$FLAG_NOBRANCH" = true ] && return

  echo -e "\n${S_NOTICE}Creating new release branch..."

  BRANCH_MSG=`git branch "${REL_PREFIX}${V_USR_INPUT}" 2>&1`
  if [ ! "$?" -eq 0 ]; then
    echo -e "\n${I_STOP} ${S_ERROR}Error\n$BRANCH_MSG\n"
    exit 1
  else
    BRANCH_MSG=`git checkout "${REL_PREFIX}${V_USR_INPUT}" 2>&1`
    echo -e "\n${I_OK} ${S_NOTICE}${BRANCH_MSG}"
  fi  
  
  # REL_PREFIX
}

# Stage & commit all files modified by this script
do-commit() {
  [ "$FLAG_NOCOMMIT" = true ] && return

  GIT_MSG+="$(get-commit-msg)" 
  echo -e "\n${S_NOTICE}Committing..."
  COMMIT_MSG=`git commit -m "${GIT_MSG}" 2>&1`
  if [ ! "$?" -eq 0 ]; then
    echo -e "\n${I_STOP} ${S_ERROR}Error\n$COMMIT_MSG\n"
    exit 1
  else
    echo -e "\n${I_OK} ${S_NOTICE}$COMMIT_MSG"
  fi  
}

# Pushes files + tags to remote repo. Changes are staged by earlier functions
do-push() {
  [ "$FLAG_NOCOMMIT" = true ] && return
  
  if [ "$FLAG_PUSH" = true ]; then
    CONFIRM="Y"
  else
    echo -ne "\n${S_QUESTION}Push tags to <${S_NORM}${PUSH_DEST}${S_QUESTION}>? [${S_NORM}N/y${S_QUESTION}]: "
    read CONFIRM  
  fi

  case "$CONFIRM" in
    [yY][eE][sS]|[yY] )
      echo -e "\n${S_NOTICE}Pushing files + tags to <${S_NORM}${PUSH_DEST}${S_NOTICE}>..."
      PUSH_MSG=`git push "${PUSH_DEST}" v"$V_USR_INPUT" 2>&1` # Push new tag
      if [ ! "$?" -eq 0 ]; then
        echo -e "\n${I_STOP} ${S_WARN}Warning\n$PUSH_MSG"
        # exit 1
      else
        echo -e "\n${I_OK} ${S_NOTICE}$PUSH_MSG"
      fi  
    ;;
  esac  
}

#### Initiate Script ###########################

check-commits-exist

# Process and prepare
process-arguments "$@"
process-version

check-branch-exist
check-tag-exists

echo -e "\n${S_LIGHT}––––––"

# Update files
bump-json-files
do-versionfile
# do-changelog
# do-branch
do-commit
# tag "${V_USR_INPUT}" "${REL_NOTE}"
do-push

echo -e "\n${S_LIGHT}––––––"
echo -e "\n${I_OK} ${S_NOTICE}"Bumped $([ -n "${V_PREV}" ] && echo "${V_PREV} –>" || echo "to ") "$V_USR_INPUT"
echo -e "\n${GREEN}Done ${I_END}\n"