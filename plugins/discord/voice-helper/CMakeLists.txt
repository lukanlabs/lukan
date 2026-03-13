cmake_minimum_required(VERSION 3.15)
project(discord-voice-helper LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(DPP REQUIRED)

add_executable(voice-helper main.cpp)
target_link_libraries(voice-helper PRIVATE dpp)
