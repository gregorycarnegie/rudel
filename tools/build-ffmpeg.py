#!/usr/bin/env python3
"""Build Rudel's pinned static FFmpeg libraries (once, before cargo build)."""
import json
import platform
import shutil
import subprocess
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VCPKG = ROOT / "target/ffmpeg-vcpkg"
TRIPLETS = {
    "x86_64-pc-windows-msvc": "x64-windows-static-md",
    "x86_64-unknown-linux-gnu": "x64-linux",
    "aarch64-apple-darwin": "arm64-osx",
    "x86_64-apple-darwin": "x64-osx",
    "aarch64-unknown-linux-gnu": "arm64-linux",
}


def run(*args, cwd=ROOT):
    subprocess.run([str(a) for a in args], cwd=cwd, check=True)


def main():
    host = next(line.split(": ", 1)[1] for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    triplet = TRIPLETS[host]
    revision = json.loads((ROOT / "vcpkg.json").read_text())["builtin-baseline"]
    if not (VCPKG / ".git").exists():
        run("git", "clone", "--no-checkout", "--filter=blob:none",
            "https://github.com/microsoft/vcpkg.git", VCPKG)
    if subprocess.run(["git", "cat-file", "-e", revision], check=False, cwd=VCPKG,
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
        run("git", "fetch", "--depth=1", "origin", revision, cwd=VCPKG)
    run("git", "checkout", revision, cwd=VCPKG)
    windows = platform.system() == "Windows"
    executable = VCPKG / ("vcpkg.exe" if windows else "vcpkg")
    if not executable.exists():
        run(VCPKG / "bootstrap-vcpkg.bat", "-disableMetrics") if windows else run(
            "sh", VCPKG / "bootstrap-vcpkg.sh", "-disableMetrics")
    # Rust's dev/release profiles can both link these optimised C libraries.
    # Keep the standard static triplet, avoiding a second, unused debug build.
    overlay = ROOT / "target/ffmpeg-triplets"
    overlay.mkdir(parents=True, exist_ok=True)
    source = VCPKG / "triplets" / (triplet + ".cmake")
    if not source.exists():
        source = VCPKG / "triplets/community" / (triplet + ".cmake")
    settings = source.read_text() + "\nset(VCPKG_BUILD_TYPE release)\n"
    if windows:
        # clang-cl uses the MSVC ABI/SDK, and avoids older MSVC's pathological
        # optimisation time on FFmpeg's large legacy codec functions.
        clang = shutil.which("clang-cl") or "C:/Program Files/LLVM/bin/clang-cl.exe"
        if not Path(clang).is_file():
            raise RuntimeError("Install LLVM (clang-cl and libclang) before building FFmpeg")
        toolchain = overlay / "clang.cmake"
        toolchain.write_text(f'set(CMAKE_C_COMPILER "{Path(clang).as_posix()}")\n'
                             f'set(CMAKE_CXX_COMPILER "{Path(clang).as_posix()}")\n'
                             f'include("{VCPKG.as_posix()}/scripts/toolchains/windows.cmake")\n')
        settings += 'set(VCPKG_LOAD_VCVARS_ENV ON)\n'
        # Only FFmpeg itself: meson drives clang-cl as a linker and hands it
        # bare /LIBPATH: options, so dav1d stays on MSVC's own toolchain.
        settings += 'if(PORT STREQUAL "ffmpeg")\n'
        settings += f'  set(VCPKG_CHAINLOAD_TOOLCHAIN_FILE "{toolchain.as_posix()}")\n'
        settings += f'  set(ENV{{PATH}} "{Path(clang).parent.as_posix()};$ENV{{PATH}}")\n'
        settings += 'endif()\n'
    elif not shutil.which("nasm"):
        # vcpkg downloads its own only on Windows; dav1d's assembly needs it.
        raise RuntimeError("Install nasm before building FFmpeg (apt install nasm / brew install nasm)")
    (overlay / source.name).write_text(settings)
    run(executable, "install", f"--triplet={triplet}", f"--host-triplet={triplet}",
        f"--overlay-triplets={overlay}", f"--x-install-root={ROOT / 'target/ffmpeg'}",
        "--disable-metrics", "--binarysource=clear")
    # Cache this with the installed libraries. Releases offer the actual
    # patched native sources beside the binary, not just an upstream URL.
    archive = ROOT / "target/ffmpeg-sources.tar.gz"
    current = False
    if archive.exists():
        with tarfile.open(archive) as sources:
            current = (sources.extractfile("vcpkg.json").read() == (ROOT / "vcpkg.json").read_bytes()
                       and sources.extractfile("tools/build-ffmpeg.py").read() == Path(__file__).read_bytes())
    if not current:
        temporary = archive.with_suffix(".tmp")
        with tarfile.open(temporary, "w:gz") as sources:
            for port in ["ffmpeg", "dav1d", "zlib"] + ([] if windows else ["openssl"]):
                source = VCPKG / "buildtrees" / port / "src"
                if not source.exists():
                    raise RuntimeError(f"Missing {port} source; rebuild with --binarysource=clear")
                sources.add(source, arcname=port)
                sources.add(VCPKG / "ports" / port, arcname=f"vcpkg-ports/{port}")
            sources.add(ROOT / "vcpkg.json", arcname="vcpkg.json")
            sources.add(Path(__file__), arcname="tools/build-ffmpeg.py")
        temporary.replace(archive)


if __name__ == "__main__":
    main()
