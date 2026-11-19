/// Object file library
/// GOAL object files are divided into segments which can be loaded by the runtime into separate
/// allocations: top-level, main, and debug. The top-level segment contains code needed to execute
/// the "top level" of a file: statements that appear outside of any function definition. This
/// segment only runs once and can be discarded to save memory.
/// Each segment has link data which is consumed by the linker at runtime to fix up pointer
/// relocations. It can insert links to data (possibly in other segments of the same object) or to
/// symbols and types. The linker can patch up MIPS instruction sequences emitted by the compiler
/// to load the correct offset or constant.

/// This library exposes data in the user-friendly ObjectFile, Segment types,
/// handling reading and writing from file, and dumping to a text file similar to objdump for
/// manual inspection.

/// Modify 4-byte word at src_offset to be a pointer to dst_offset within the same segment.
pub struct PointerLink {
    pub src_offset: u32,
    pub dst_offset: u32,
}

/// Modify a lui/ori instruction pair to get the address of dst_offset in dst_seg. The lo_hi_offset
/// is the distance between lui and ori in words, typically 1.
pub struct SplitPointerLink {
    pub src_offset: u32,
    pub dst_offset: u32,
    pub dst_seg: u8,
    pub lo_hi_offset: u8,
}

/// Modify addresses to point to a type with the given number of methods.
/// Since the linker may encounter this before the GOAL runtime has constructed the type, you must
/// tell it how many methods the type has, so it can allocate memory for the type and its vtable if
/// none exists yet.
pub struct TypeLinks {
    pub name: String,
    pub offsets: Vec<u32>,
    pub num_methods: u8,
}

/// Modify word to point to symbol. This can either modify an entire 4-byte word to be a symbol
/// pointer, or can modify the lower 16-bits of an instruction to insert the offset between the
/// symbol and the s7 register. The first is used for static data containing symbols, the second is
/// used for code that uses a symbol directly (known at compile time).
pub struct SymbolLink {
    pub offset: u32,
    pub offset_from_st: bool,
}

/// All links to the symbol with the given name.
pub struct SymbolLinks {
    pub name: String,
    pub links: Vec<SymbolLink>,
}

/// The data and link information of a segment.
pub struct Segment {
    pub pointers: Vec<PointerLink>,
    pub split_pointers: Vec<SplitPointerLink>,
    pub symbols: Vec<SymbolLinks>,
    pub types: Vec<TypeLinks>,
    pub data: Vec<u8>,
}

/// Objectfile is a collection of segments
pub struct ObjectFile {
    pub name: String,
    pub segments: Vec<Segment>,
}

pub const MAIN_SEGMENT: usize = 0;
pub const DEBUG_SEGMENT: usize = 1;
pub const TOP_LEVEL_SEGMENT: usize = 2;

pub fn segment_nickname(seg_id: usize) -> Option<&'static str> {
    match seg_id {
        TOP_LEVEL_SEGMENT => Some("top-level"),
        MAIN_SEGMENT => Some("main"),
        DEBUG_SEGMENT => Some("debug"),
        _ => None,
    }
}

/// Get a slice of length N from the front of input and modify input to be the remaining.
fn take<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], String> {
    if input.len() < N {
        return Err(format!(
            "Unexpected end of data. Tried to read {} bytes but only {} are available.",
            N,
            input.len()
        ));
    }

    let (head, tail) = input.split_at(N);
    *input = tail;
    Ok(head.try_into().unwrap())
}

/// Get a u32 from the front of input and modify input to be the remaining.
fn read_u32(input: &mut &[u8]) -> Result<u32, String> {
    Ok(u32::from_le_bytes(take(input)?))
}

/// Get the position of the first null character in a fixed-length char[] buffer.
/// Verifies that the remaining characters are also null.
fn first_null(input: &[u8]) -> Result<usize, String> {
    let mut result: Option<usize> = None;
    for (i, &byte) in input.iter().enumerate() {
        if byte == 0 {
            if result.is_none() {
                result = Some(i);
            }
        } else {
            if result.is_some() {
                return Err("got characters after null.".to_string());
            }
        }
    }
    result.ok_or("missing null terminator.".to_string())
}

/// File format: Location of data and link table for a segment within an object.
#[derive(Debug)]
struct SegmentInfo {
    link_offset: u32,
    data_offset: u32,
    data_size: u32,
    flag: u32, // always 0
}

impl SegmentInfo {
    /// Parse file bytes into struct.
    pub fn parse(input: &mut &[u8]) -> Result<Self, String> {
        let result = Self {
            link_offset: read_u32(input)?,
            data_offset: read_u32(input)?,
            data_size: read_u32(input)?,
            flag: read_u32(input)?,
        };
        if result.flag != 0 {
            return Err(format!("invalid flag {:x} in SegmentInfo", result.flag));
        }
        Ok(result)
    }
}

/// Read a byte from the link stream, returning error if it's past the end.
fn get_link(link: &[u8], offset: usize, ctxt: &str) -> Result<u8, String> {
    Ok(*link
        .get(offset)
        .ok_or_else(|| format!("unexpected end of link data: {ctxt}"))?)
}

/// Read the first half of the link data, pointer links, validate them, and return them. The link
/// data is in a stream that allows the runtime linker to easily split up the work over multiple
/// calls to the linker, but is inconvenient for analysis.
fn read_pointer_links(
    data: &[u8],
    link: &[u8],
    segment_lengths: &[usize],
) -> Result<(Vec<PointerLink>, Vec<SplitPointerLink>, usize), String> {
    // if the pointer section starts with a terminator, is the terminator for the whole section,
    // not a terminator for the fix/seek machine.
    if get_link(link, 0, "first pointer link byte")? == 0 {
        return Ok((vec![], vec![], 1));
    }

    // To avoid conflict with an offset of 0 and the null terminator
    // in the link stream, start the data pointer with an offset of
    // -4 so the link stream uses 1 to adjust the first word.
    let mut data_offset: isize = -4;
    let mut fixing = false;
    let mut link_offset = 0;
    let mut pointers: Vec<PointerLink> = vec![];
    let mut split_pointers: Vec<SplitPointerLink> = vec![];

    loop {
        // over pointers
        // over repeated links
        let link_byte: u8 = get_link(link, link_offset, "pointer links")?;
        link_offset += 1;
        if !fixing {
            // seek to next group
            data_offset += 4 * link_byte as isize;
        } else {
            for _ in 0..link_byte {
                // read the code we're replacing.
                let code = data
                    .get(data_offset as usize..data_offset as usize + 4)
                    .ok_or("pointer link source out")?;

                if code[3] == 0 {
                    // link entire word as 32-bit pointer.
                    let dst_offset = u32::from_le_bytes(code.try_into().unwrap());
                    if dst_offset as usize > data.len() {
                        return Err(format!(
                            "Pointer link to {} exceeds segment size {}",
                            dst_offset,
                            data.len()
                        ));
                    }
                    pointers.push(PointerLink {
                        src_offset: data_offset as u32,
                        dst_offset: dst_offset,
                    });
                } else {
                    // link the upper and lower 16-bits of the address in separate words,
                    // suitable for lui/ori pair.
                    let dst_seg = code[1] & 0xf;
                    if dst_seg as usize >= segment_lengths.len() {
                        return Err(format!(
                            "Split pointer link to segment {} exceeds segment count of {}",
                            dst_seg,
                            segment_lengths.len()
                        ));
                    }
                    let lo_hi_offset = code[1] >> 4;
                    if lo_hi_offset == 0 {
                        return Err("Split pointer lo-hi offset is 0".to_string());
                    }
                    assert_eq!(lo_hi_offset, 1);

                    let lo_code_offset = data_offset as usize + 4 * lo_hi_offset as usize;
                    let lo_code = u32::from_le_bytes(
                        data.get(lo_code_offset..lo_code_offset + 4)
                            .ok_or("pointer link source out")?
                            .try_into()
                            .unwrap(),
                    );
                    let offset = ((lo_code & 0xffff) as usize) + ((code[0] as usize) << 16);
                    if offset >= segment_lengths[dst_seg as usize] {
                        return Err(format!(
                            "Pointer link to {} exceeds segment {} size {}",
                            offset, dst_seg, segment_lengths[dst_seg as usize]
                        ));
                    }

                    split_pointers.push(SplitPointerLink {
                        src_offset: data_offset as u32,
                        dst_offset: offset as u32,
                        dst_seg: dst_seg,
                        lo_hi_offset: lo_hi_offset,
                    });
                }
                data_offset += 4;
            }
        }

        // By default, toggle modes after each byte.
        // If the number of things to process in the mode exceeds
        // 0xff, then keep emitting bytes with the count.
        // If the number of things to process is 0xff, emit 0xff then 0x0

        if link_byte == 0xff {
            if get_link(link, link_offset, "pointer continue")? == 0 {
                // exactly 0xff, change modes
                link_offset += 1;
                fixing = !fixing;
            } // otherwise, stay in mode.

            continue;
        }

        // change modes
        fixing = !fixing;

        // check for terminator.
        if get_link(link, link_offset, "pointer terminator")? == 0 {
            return Ok((pointers, split_pointers, link_offset + 1));
        }
    }
}

/// Read a name (null-terminated UTF8 string) from the link table. Return number of bytes to
/// advance, and the string.
fn get_link_name(link: &[u8]) -> Result<(usize, String), String> {
    let end = link
        .iter()
        .position(|x| *x == 0)
        .ok_or("link name missing null")?;
    let s = std::str::from_utf8(&link[..end])
        .map_err(|x| format!("invalid utf8 in link name: {}", x))?;
    Ok((end + 1, s.to_owned()))
}

// Read the offsets of the data to patch to refer to the given symbol.
fn get_symbol_link_offsets(link: &[u8]) -> Result<(usize, Vec<u32>), String> {
    let mut data_offset = 0;
    let mut link_offset = 0;
    let mut result: Vec<u32> = vec![];

    loop {
        loop {
            let count = get_link(link, link_offset, "symbol integers")?;
            link_offset += 1;
            data_offset += count as u32 * 4;
            if count != 0xff {
                break;
            }
        }

        result.push(data_offset);

        let next_count = get_link(link, link_offset, "symbol integers terminator")?;
        if next_count == 0 {
            return Ok((link_offset + 1, result));
        }
    }
}

// Read symbol and type links
fn read_symbols_links(
    data: &[u8],
    link: &[u8],
) -> Result<(usize, Vec<SymbolLinks>, Vec<TypeLinks>), String> {
    let mut link_offset = 0;
    let mut symbols: Vec<SymbolLinks> = vec![];
    let mut types: Vec<TypeLinks> = vec![];

    loop {
        let link_byte = get_link(link, link_offset, "symbols next")?;
        if link_byte == 0 {
            break;
        }

        // if the upper bit is set, then it is the method count in the lower 7 bits.
        // otherwise, it's the first character of the name.
        let is_type = (link_byte & 0x80) != 0;
        let method_count = link_byte & 0x7f; // only meaningful for type.
        if is_type {
            link_offset += 1;
        }

        let (seek, name) = get_link_name(link.get(link_offset..).ok_or("name of symbol")?)?;
        link_offset += seek;
        let (seek, offsets) =
            get_symbol_link_offsets(link.get(link_offset..).ok_or("symbol link locs")?)?;
        link_offset += seek;

        if is_type {
            types.push(TypeLinks {
                name: name,
                offsets: offsets,
                num_methods: method_count,
            });
        } else {
            let links = offsets
                .iter()
                .map(|&x| -> Result<SymbolLink, &'static str> {
                    let bytes = data
                        .get(x as usize..)
                        .and_then(|rest| rest.first_chunk::<4>())
                        .ok_or("invalid offset in symbol link")?;
                    let code = u32::from_le_bytes(*bytes);

                    let offset = code != 0xffffffff;
                    if offset {
                        if (code & 0xffff) != 0 {
                            return Err("invalid");
                        }
                    }

                    Ok(SymbolLink {
                        offset: x,
                        offset_from_st: offset,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            symbols.push(SymbolLinks {
                name: name,
                links: links,
            });
        }
    }
    Ok((link_offset, symbols, types))
}

fn read_segment(data: &[u8], link: &[u8], segment_lengths: &[usize]) -> Result<Segment, String> {
    let mut linkp = link;

    let (ptrs, split_ptrs, offset) = read_pointer_links(data, linkp, segment_lengths)?;
    linkp = &linkp[offset..];

    let (offset, syms, types) = read_symbols_links(data, linkp)?;
    linkp = &linkp[offset..];

    if linkp.len() >= 16 {
        return Err(format!("got {} bytes after link stream ended", linkp.len()));
    }

    if !linkp.iter().all(|&x| x == 0) {
        return Err(format!(
            "got nonzero bytes after link stream ended: {:?}",
            linkp
        ))?;
    }

    Ok(Segment {
        pointers: ptrs,
        split_pointers: split_ptrs,
        symbols: syms,
        types: types,
        data: data.to_owned(),
    })
}

impl ObjectFile {
    pub fn read(input: &[u8]) -> Result<Self, String> {
        let file_len = input.len();

        // Read the file header.
        let mut p = input;
        let type_tag = read_u32(&mut p)?;
        if type_tag != 0 {
            return Err(format!(
                "invalid type_tag bytes {type_tag:x} in ObjectHeaderV3"
            ));
        }
        let link_length = read_u32(&mut p)?;
        let version = read_u32(&mut p)?;
        if version != 3 {
            return Err(format!(
                "invalid version {version} in ObjectHeaderV3 (expected 3)"
            ));
        }
        let num_segments = read_u32(&mut p)? as usize;
        if num_segments != 3 {
            return Err(format!(
                "invalid number of segments {num_segments} in ObjectHeaderV3 (expected 3)"
            ));
        }
        let name = take::<64>(&mut p)?;
        let name_end = first_null(&name)?;
        let segs = (0..num_segments)
            .map(|_| SegmentInfo::parse(&mut p))
            .collect::<Result<Vec<_>, _>>()?;

        // check sizes
        let total_data_size: u32 = segs.iter().map(|x| x.data_size.next_multiple_of(16)).sum();
        if total_data_size + link_length != file_len as u32 {
            return Err(format!(
                "Object file invalid size: {total_data_size} + {link_length} != {file_len}"
            ));
        }

        let header_size = input.len() - p.len();

        // get slices containing the link and data for each segment.
        struct SegmentData<'a> {
            link: &'a [u8],
            data: &'a [u8],
        }
        let data: Vec<SegmentData> = segs.iter().enumerate().map(|(i, seg)| -> Result<SegmentData, String> {
            let link_start = seg.link_offset as usize;
            if link_start < header_size {
                return Err(format!("seg[{i}] link starts at {link_start}, in obj header ending at {header_size}"));
            }

            let data_start = (link_length + seg.data_offset) as usize;
            let data_end = data_start + seg.data_size as usize;
            let expected_data_end = segs.get(i + 1).map_or(file_len, |next| (next.data_offset + link_length) as usize);

            if data_end.next_multiple_of(16) != expected_data_end {
                return Err(format!("seg[{i}] data expected to end at {expected_data_end}, got align16({data_end})"));
            }

            let link_end = segs.get(i + 1).map_or(link_length, |next| next.link_offset) as usize;
            if link_end <= link_start {
                return Err(format!("seg[{i}] link data starts at {link_start} and ends at {link_end}"));
            }
            Ok(SegmentData{link: &input[link_start..link_end], data: &input[data_start..data_end]})
        }).collect::<Result<Vec<_>,_>>()?;

        let seg_lengths = data.iter().map(|x| x.data.len()).collect::<Vec<_>>();

        Ok(Self {
            name: std::str::from_utf8(&name[..name_end])
                .map_err(|x| format!("name not UTF-8: {}", x))?
                .to_owned(),
            segments: data
                .iter()
                .map(|x| -> Result<Segment, String> { read_segment(x.data, x.link, &seg_lengths) })
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

