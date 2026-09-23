# Reaching npm from a bash script on Windows. Sourced, not run.
#
# npm has to go through the Windows toolchain either way, but the two dev
# shells need opposite spellings, and each one fails *silently* in the other's:
#
#   WSL:       `cmd.exe /c "npm ..."`. npm.cmd is on the PATH here too, but
#              bash tries to run it as a shell script and dies on its line 13.
#   Git Bash:  `npm.cmd` directly. MSYS rewrites cmd.exe's `/c` switch into a
#              path before cmd ever sees it, so cmd opens, runs nothing, and
#              exits 0 -- a build that "succeeds" without building.
#
# That second case is why this exists: check.sh was reporting "all checks
# passed" while skipping both frontend steps, and release.sh was leaving a
# stale exe on disk while printing the path to it as if it were fresh.

npm_run() {
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) npm.cmd "$@" ;;
    *) cmd.exe /c "npm $*" ;;
  esac
}
