#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
preset=snow-shot-linux-x64-debug
if [[ "${1:-}" == --help ]]; then
    echo 'Usage: build-linux.sh [preset] [--target TARGET] [--parallel JOBS] [--clean] [--skip-bootstrap] [-- CMAKE_OPTIONS...]'
    exit 0
fi
if [[ $# -gt 0 && "$1" != --* ]]; then
    preset="$1"
    shift
fi
targets=()
parallelism=''
clean=0
skip_bootstrap=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target) [[ $# -ge 2 ]] || { echo '--target needs a target' >&2; exit 1; }; targets+=("$2"); shift 2 ;;
        --parallel) [[ $# -ge 2 ]] || { echo '--parallel needs a positive integer' >&2; exit 1; }; parallelism="$2"; shift 2 ;;
        --clean) clean=1; shift ;;
        --skip-bootstrap) skip_bootstrap=1; shift ;;
        --) shift; break ;;
        *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
done
if [[ "$skip_bootstrap" == 0 ]]; then
    "$root/scripts/bootstrap-linux.sh" --skip-dependency-install
fi
build_dir="$root/build/$preset"
if [[ "$clean" == 1 && -e "$build_dir" ]]; then
    case "$build_dir" in
        "$root"/build/snow-shot-linux-*) rm -rf -- "$build_dir" ;;
        *) echo "Refusing to remove an unexpected build directory: $build_dir" >&2; exit 1 ;;
    esac
fi
cd "$root"
cmake --preset "$preset" "$@"
if [[ ${#targets[@]} == 0 ]]; then
    targets=(snow_shot)
fi
build=(--build --preset "build-$preset" --target "${targets[@]}")
if [[ -n "$parallelism" ]]; then
    build+=(--parallel "$parallelism")
fi
cmake "${build[@]}"
