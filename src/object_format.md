# GOAL Object File Format

Jak 1 puts all code in v3 object files. These files always have exactly three segments: main, debug, and top-level. Each segment has its own linking data and can be loaded into different heaps, but segments can still reference data in other segments. The linker format is designed for an incremental linker that can save its state and resume.

The top-level segment contains code that runs once when the object file is loaded. It contains all code outside of functions, typically function and type definitions themselves, which generate a small amount of code to set the value of the symbol. It can also contain initialization, typically for generating tables, creating singletons, etc. The linker can discard the top-level segment without discarding the allocations made by the top-level segment, making it a convenient place to store run-once initialization and allocation logic.

The main segment has all other function and data, excluding functions and data explicitly marked debug, which go in the debug segment. As you'd expect, the debug segment is only loaded when the game is in debug mode.

The linker supports:
- Pointer links: patch a 4-byte word to an address in the segment.
- Split pointer links: patch the low 2-bytes of a word to point to the low or high 16-bits of an address. This is used to patch `lui`, `ori` instruction pairs to load an address. Unlike plain pointer links, this can reference data in another segment.
- Type links: patch a 4-byte word to point to a type object. This is used to insert type tags on statically defined objects. The type object contains the vtable and is a different size depending on the number of methods. Since the type object may not exist yet, the object file stores the maximum number of methods the type may have, so the linker can allocate space for the vtable when it encounters a type that is not yet allocated.
- Symbol links: patch a 4-byte word to point to a symbol in the symbol table. This is used for symbols in static objects.
- Symbol-table relative links: patch an instruction's immediate to the offset of a symbol relative to the symbol table register `s7`. This allows code to load the symbol value or address in a single instruciton. This is used for code reading global variables, including functions, and for symbols-used-as-enums, like `'done`.

# Object file disassembly format

The `objdump` binary validates object file data and disassembles it. The validation is intended to catch bugs in our object file writer. The disassembly output is useful to diff to figure out why an object file isn't matching.

The binary can be run through Rust Cargo:
```
 cargo run --bin objdump -- file.o
```

For more advanced analysis, use OpenGOAL's decompiler.

Labels for code and data look like `labelname:`. Default label names are `L1:`, `L2:`, ...

## Directives

### `.segment name`

Marks the start of a new segment. Name can be one of `top-level`, `main`, or `debug`.

### `.type-methods <typename> <method-count>`

Declares `typename` to have `method-count` methods or fewer. This goes into linker metadata for the runtime to allocate memory for the type's vtable if the type is seen for the first time when linking.

## Data

### `.type <typename>`

Insert a 4-byte reference to a type. The linker patches this to point to the runtime type object, allocating it at link time (but not initializing it) if the type doesn't exist yet.

### `.word <value>`

Insert a 4-byte word. Value is hexadecimal.
