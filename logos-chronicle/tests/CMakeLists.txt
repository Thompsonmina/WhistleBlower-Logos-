cmake_minimum_required(VERSION 3.14)
project(ChronicleTests LANGUAGES CXX)

include(LogosTest)

logos_test(
    NAME chronicle_tests
    MODULE_SOURCES ../src/chronicle_helpers.cpp
    TEST_SOURCES
        main.cpp
        test_chronicle.cpp
)
