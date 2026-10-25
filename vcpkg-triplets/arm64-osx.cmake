# vcpkg's stock arm64-osx triplet plus the deployment target, so the libraries run on the same macOS releases
# as ImGit itself (CMAKE_OSX_DEPLOYMENT_TARGET in CMakeLists.txt; keep the two in step).
set(VCPKG_TARGET_ARCHITECTURE arm64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE static)

set(VCPKG_CMAKE_SYSTEM_NAME Darwin)
set(VCPKG_OSX_ARCHITECTURES arm64)
set(VCPKG_OSX_DEPLOYMENT_TARGET 12.0)
