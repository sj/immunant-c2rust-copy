# C2Rust

## Translation

The `ast-exporter` extracts from a C file the abstract syntax tree and type information produced by
Clang and serializes it into CBOR files. The `ast-importer` consumes these CBOR files and generates
Rust source code preserving the semantics (as understood under C99) of the initial C program.

The translated Rust files will not depend directly on each other like
normal Rust modules. They will export and import functions through the C
API. These modules can be compiled together into a single static Rust
library.

There are several [known limitations](docs/known-limitations.md)
in this translator. Some of these restrictions come from limitations of
Rust and others come from complexities of the features themselves. The
translator will attempt to skip function definitions that use
unsupported features.

### Setting up a build environment

There are three ways to build the C2Rust project:

1. In the provided vagrant environment. See the [vagrant README](vagrant/README.md)
2. In the provided docker environment. See the [docker README](docker/README.md)
3. Building directly on a macOS or Linux host. The previous two options automatically install all pre-requisites during provisioning. With this option, prerequisites must be installed manually. 
    - If you are on a Debian-based OS, you can run `provision_deb.sh` to do so. 
    - If you are on macOS, install the Xcode command-line tools (e.g., `xcode-select --install`) and [homebrew](https://brew.sh/) first. Then run `provision_mac.sh`.
   
*NOTE*: The translator supports both macOS and Linux. Other features, such as cross checking the functionality between C and Rust code, are currently limited to Linux hosts. 

### Building

These two projects have some large dependencies (namely parts of LLVM and Clang). If 
you've installed  the necessary tools, the following should build `ast-exporter` and 
`ast-importer` and all of their dependencies, automatically pulling them in if 
necessary.

Building from scratch takes on the order of 30 minutes. The script has been tested on recent versions of macOS and Ubuntu.

    $ ./scripts/build_translator.py

To manually build the `ast-exporter`, check out [these build instructions][0]. To manually build the
`ast-importer`, check out [its README](ast-importer/README.md).

### Testing

Tests are found in the [`tests`](tests) folder. If both the `ast-exporter` and `ast-importer` are
built, you should be able to run the tests with

    $ ./scripts/test_translator.py tests

This basically tests that the original C file and translated Rust file produce the same output when
compiled and run. More details about tests are in [this README](tests/README.md).

 [0]: docs/building-ast-exporter.md

### Using the translator

The C2Rust translation process relies use Clang to parse and type-check
input C files. For Clang to do this it needs to know information that is
passed in via command-line flags. This information can be found in an
automatically generated `compile_commands.json`.

The `compile_commands.json` file can be automatically create using
either `cmake`, `intercept-build`, or `bear` (Linux only).

#### Generating `compile_commands.json` with `cmake`

When creating the initial build directory with cmake specify
`-DCMAKE_EXPORT_COMPILE_COMMANDS=1`. This only works on projects
configured to be built by cmake. This works on Linux and MacOS.

    $ mkdir build
    $ cd build
    $ cmake -DCMAKE_EXPORT_COMPILE_COMMANDS=1 ..

#### Generating `compile_commands.json` with `intercept-build`

Intercept build is distributed with clang and recommended for makefile projects on macOS.

	$ intercept-build make
	$ intercept-build xcodebuild

#### Generating `compile_commands.json` with `bear`

When building on Linux, *Bear* is automatically build by the
`build_translator.py` script and installed into the `dependencies`
directory.

    $ ./configure CC=clang
    $ bear make

#### Translating source files

The `transpile.py` script will automatically translate all of the C
source files mentioned in the previously generated
`compile_commands.json`.

    $ scripts/transpile.py ./compile_commands.json

## Acknowledgements and Licensing

This material is available under the BSD-3 style license as found in the
`LICENSE` file.

The C2Rust translator is inspired by Jamey Sharp's [Corrode](https://github.com/jameysharp/corrode) translator. We rely on 
[Emscripten](https://github.com/kripken/emscripten)'s 
Relooper algorithm to translate arbitrary C control flows.

This material is based upon work supported by the United States Air Force and
DARPA under Contract No. FA8750-15-C-0124.  Any opinions, findings and
conclusions or recommendations  expressed in this material are those of the
author(s) and do not necessarily reflect the views of the United States Air
Force and DARPA.  Distribution Statement A, “Approved for Public Release,
Distribution Unlimited.”
