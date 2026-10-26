# The unit tests: `cmake --preset <preset> -DIMGIT_BUILD_TESTS=ON`, build, then `ctest --test-dir build/<preset>`.
find_package(GTest CONFIG REQUIRED)

add_executable(imgit_tests
    syntax_test.cpp)
target_link_libraries(imgit_tests PRIVATE imgit_syntax GTest::gtest GTest::gtest_main)
if(MSVC)
    target_compile_options(imgit_tests PRIVATE /utf-8 /EHsc /W4 /WX)
else()
    target_compile_options(imgit_tests PRIVATE -Wall -Wextra -Wpedantic -Werror)
endif()

include(GoogleTest)
gtest_discover_tests(imgit_tests)
