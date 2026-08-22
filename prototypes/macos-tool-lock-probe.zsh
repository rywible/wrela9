#!/bin/zsh -f

# THROWAWAY PROTOTYPE. This probes a local macOS installation; it is not production tooling.
set -eu

die() {
  print -u2 -- "tool-lock prototype: $*"
  exit 1
}

usage() {
  print -u2 -- "usage:"
  print -u2 -- "  $0 capture <qemu-path> <lld-path> <lock-path>"
  print -u2 -- "  $0 verify <lock-path>"
  exit 2
}

require_host_tools() {
  local command_name
  for command_name in awk diff file grep otool realpath shasum sort sw_vers uname; do
    command -v "$command_name" >/dev/null || die "required host command is missing: $command_name"
  done
}

validate_explicit_tool() {
  local role=$1
  local requested=$2

  [[ "$requested" == /* ]] || die "$role path must be absolute: $requested"
  [[ "$requested" != *$'\n'* && "$requested" != *$'\t'* ]] || die "$role path contains an unsupported tab or newline"
  [[ -e "$requested" ]] || die "$role tool is missing: $requested"
  [[ -x "$requested" ]] || die "$role tool is not executable: $requested"
  file "$requested" | grep -q 'Mach-O 64-bit executable arm64' || die "$role tool is not an arm64 Mach-O executable: $requested"
}

collect_tool() {
  local role=$1
  local requested=$2
  local canonical
  local alias_target="-"
  local current loader_dir root_dir dependency resolved suffix rpath
  local -a queue dependencies rpaths rows sorted_rows
  local -A seen system_seen

  validate_explicit_tool "$role" "$requested"
  canonical=$(realpath "$requested")
  [[ -L "$requested" ]] && alias_target=$(readlink "$requested")

  print -r -- $'tool\t'"$role"$'\trequested\t'"$requested"
  print -r -- $'tool\t'"$role"$'\tcanonical\t'"$canonical"
  print -r -- $'tool\t'"$role"$'\talias-target\t'"$alias_target"

  queue=("$canonical")
  rows=()
  while (( ${#queue[@]} )); do
    current=${queue[1]}
    queue=("${queue[@]:1}")
    current=$(realpath "$current")
    [[ -n ${seen[$current]-} ]] && continue
    seen[$current]=1

    rows+=($'file\t'"$role"$'\t'"$current"$'\t'"$(shasum -a 256 "$current" | awk '{print $1}')")
    loader_dir=${current:h}
    root_dir=${canonical:h}
    rpaths=("${(@f)$(otool -l "$current" | awk '/cmd LC_RPATH/{getline; getline; print $2}')}" )
    dependencies=("${(@f)$(otool -L "$current" | tail -n +2 | sed -E 's/^[[:space:]]*([^[:space:]]+).*/\1/')}" )

    for dependency in "${dependencies[@]}"; do
      [[ -z "$dependency" ]] && continue
      resolved=""
      case "$dependency" in
        /System/*|/usr/lib/*)
          system_seen[$dependency]=1
          rows+=($'edge\t'"$role"$'\t'"$current"$'\t'"$dependency"$'\tsystem')
          continue
          ;;
        @loader_path/*)
          resolved="$loader_dir/${dependency#@loader_path/}"
          ;;
        @executable_path/*)
          resolved="$root_dir/${dependency#@executable_path/}"
          ;;
        @rpath/*)
          suffix=${dependency#@rpath/}
          for rpath in "${rpaths[@]}"; do
            [[ -z "$rpath" ]] && continue
            rpath=${rpath//@loader_path/$loader_dir}
            rpath=${rpath//@executable_path/$root_dir}
            if [[ -e "$rpath/$suffix" ]]; then
              resolved="$rpath/$suffix"
              break
            fi
          done
          ;;
        /*)
          resolved=$dependency
          ;;
      esac

      [[ -n "$resolved" && -e "$resolved" ]] || die "$role dependency cannot be resolved: $current -> $dependency"
      resolved=$(realpath "$resolved")
      rows+=($'edge\t'"$role"$'\t'"$current"$'\t'"$dependency"$'\t'"$resolved")
      queue+=("$resolved")
    done
  done

  sorted_rows=("${(@on)rows}")
  for current in "${sorted_rows[@]}"; do
    print -r -- "$current"
  done
  print -r -- $'closure\t'"$role"$'\texternal-files\t'"${#seen[@]}"
  print -r -- $'closure\t'"$role"$'\tsystem-names\t'"${#system_seen[@]}"
  print -r -- $'closure\t'"$role"$'\tsha256\t'"$(printf '%s\n' "${sorted_rows[@]}" | shasum -a 256 | awk '{print $1}')"
}

capture_manifest() {
  local qemu_path=$1
  local lld_path=$2

  print -r -- $'format\t1'
  print -r -- $'host\tarchitecture\t'"$(uname -m)"
  print -r -- $'host\tmacos-version\t'"$(sw_vers -productVersion)"
  print -r -- $'host\tmacos-build\t'"$(sw_vers -buildVersion)"
  collect_tool qemu "$qemu_path"
  collect_tool lld "$lld_path"
}

capture_lock() {
  local qemu_path=$1
  local lld_path=$2
  local lock_path=$3
  local lock_parent=${lock_path:h}
  local temporary

  [[ -d "$lock_parent" ]] || die "lock parent directory does not exist: $lock_parent"
  temporary=$(mktemp "$lock_parent/.tool-lock-PROTOTYPE.XXXXXX")
  if capture_manifest "$qemu_path" "$lld_path" > "$temporary"; then
    chmod 600 "$temporary"
    mv -f "$temporary" "$lock_path"
    print -- "captured prototype lock: $lock_path"
  else
    rm -f "$temporary"
    exit 1
  fi
}

verify_lock() {
  local lock_path=$1
  local qemu_path lld_path temporary_directory observed

  [[ -f "$lock_path" ]] || die "lock is missing: $lock_path"
  qemu_path=$(awk -F $'\t' '$1 == "tool" && $2 == "qemu" && $3 == "requested" { print $4 }' "$lock_path")
  lld_path=$(awk -F $'\t' '$1 == "tool" && $2 == "lld" && $3 == "requested" { print $4 }' "$lock_path")
  [[ -n "$qemu_path" && -n "$lld_path" ]] || die "lock does not contain both explicit tool paths"

  temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/wrela-tool-lock-PROTOTYPE.XXXXXX")
  observed="$temporary_directory/observed.lock"
  capture_manifest "$qemu_path" "$lld_path" > "$observed" || die "current installation could not be captured"
  if diff -u "$lock_path" "$observed"; then
    rm -rf "$temporary_directory"
    print -- "verified prototype lock: exact installation matches"
    return 0
  fi

  rm -rf "$temporary_directory"
  print -u2 -- "tool-lock prototype: verification rejected; the diff above is the exact drift"
  return 1
}

require_host_tools
(( $# >= 1 )) || usage
operation=$1
shift

case "$operation" in
  capture)
    (( $# == 3 )) || usage
    capture_lock "$1" "$2" "$3"
    ;;
  verify)
    (( $# == 1 )) || usage
    verify_lock "$1"
    ;;
  *)
    usage
    ;;
esac
