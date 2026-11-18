# GOAL Decompilation

This project is to create a matching decompilation of GOAL code in the Jak and Daxter games. The objective is to reproduce the object files found in the games exactly, but with readable, documented code, in the style of the original. This is monstly meant as a reference for how the games worked or an entertaining thing to read, not a base for a PC port.

Currently I am working on tooling to work with GOAL object files and getting familiar with Rust, starting with a object file parser and disassembler. There is no plan to make a full decompiler. This is intended for use in a future binary diffing tool to identify differences in register allocation, alignment, etc.

