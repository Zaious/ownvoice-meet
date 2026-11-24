# Windows only (see .cargo/config.toml): llama.cpp's CMake sets the runtime through
# CMAKE_MSVC_RUNTIME_LIBRARY, which wins over the /MT flag the Rust build passes; this makes it the
# static runtime, matching the prebuilt sherpa-onnx libraries.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded" CACHE STRING "" FORCE)
