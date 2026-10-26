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

# The same tests under Valgrind memcheck (-DIMGIT_VALGRIND=ON): any invalid read or write, use of uninitialised
# memory, or definite / indirect leak fails the test. `ctest -L valgrind` runs only it, `ctest -LE valgrind` skips it.
if(IMGIT_VALGRIND)
    find_program(VALGRIND_EXECUTABLE valgrind REQUIRED)
    add_test(NAME imgit_tests.valgrind
        COMMAND ${VALGRIND_EXECUTABLE}
            --error-exitcode=1
            --leak-check=full
            --show-leak-kinds=definite,indirect
            --errors-for-leak-kinds=definite,indirect
            --track-origins=yes
            --suppressions=${CMAKE_CURRENT_SOURCE_DIR}/valgrind.supp
            $<TARGET_FILE:imgit_tests>)
    set_tests_properties(imgit_tests.valgrind PROPERTIES LABELS valgrind)
endif()
