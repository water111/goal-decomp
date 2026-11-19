use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::{env, fs, process::ExitCode};

type Label = (u8, u32); // segment index, word offset

enum WordLink {
    None,
    Type(usize),
    Pointer((u8, u32)),     // segment index, byte offset
    Lo16Pointer((u8, u32)), // segment index, byte offset
    Hi16Pointer((u8, u32)), // segment index, byte offset
}

struct LinkedWord {
    value: u32,
    link: WordLink,
    label_id: Option<u32>,
    in_function: bool,
}

fn make_data_words(seg: &goalc::Segment) -> Vec<LinkedWord> {
    seg.data
        .chunks_exact(4)
        .map(|x| LinkedWord {
            value: u32::from_le_bytes(x.try_into().unwrap()),
            link: WordLink::None,
            label_id: None,
            in_function: false,
        })
        .collect()
}

fn objdump(obj: &goalc::ObjectFile) {
    let mut out = String::new();
    let mut labels = BTreeSet::<Label>::new();
    let mut segment_words: Vec<Vec<LinkedWord>> =
        obj.segments.iter().map(make_data_words).collect();

    // insert type links, so we can find functions
    for (i, segment) in obj.segments.iter().enumerate() {
        let words = &mut segment_words[i];
        for (j, links) in segment.types.iter().enumerate() {
            for offset in &links.offsets {
                let word = &mut words[*offset as usize / 4];
                assert!(matches!(word.link, WordLink::None));
                word.link = WordLink::Type(j);
            }
        }
    }

    // find functions

    // establish labels
    for (i, segment) in obj.segments.iter().enumerate() {
        let words = &mut segment_words[i];
        for link in &segment.pointers {
            labels.insert((i as u8, link.dst_offset / 4));
            assert_eq!((link.src_offset & 3), 0);
            let word = &mut words[link.src_offset as usize / 4];
            assert!(matches!(word.link, WordLink::None));
            word.link = WordLink::Pointer((i as u8, link.dst_offset));
        }
        for link in &segment.split_pointers {
            labels.insert((link.dst_seg, link.dst_offset / 4));
            assert_eq!((link.src_offset & 3), 0);
            let word_idx = link.src_offset as usize / 4;
            let word = &mut words[word_idx];
            assert!(matches!(word.link, WordLink::None));
            word.link = WordLink::Hi16Pointer((link.dst_seg, link.dst_offset));
            words[word_idx + link.lo_hi_offset as usize].link =
                WordLink::Lo16Pointer((link.dst_seg, link.dst_offset));
        }
    }
    let label_ids: BTreeMap<Label, u32> = labels
        .into_iter()
        .enumerate()
        .map(|(i, loc)| (loc, (i + 1) as u32))
        .collect();

    // insert labels
    for (&(segment, word_idx), &label_id) in &label_ids {
        segment_words[segment as usize][word_idx as usize].label_id = Some(label_id);
    }

    // final pass to write
    for (i, segment) in obj.segments.iter().enumerate() {
        // segment name
        writeln!(out, ".segment {}", goalc::segment_nickname(i).unwrap()).unwrap();
        // type method counts
        for x in &segment.types {
            writeln!(out, ".type-methods {}, {}", x.name, x.num_methods).unwrap();
        }

        let label_id_offset = |seg_idx: u8, byte_offset: u32| -> (u32, u32) {
            let word_idx = byte_offset / 4;
            let byte_offset = byte_offset & 3;
            let label_id = *label_ids.get(&(seg_idx, word_idx)).unwrap();
            (label_id, byte_offset)
        };

        // write words!
        for word in &segment_words[i] {
            if let Some(i) = word.label_id {
                writeln!(out, "L{}:", i).unwrap();
            }

            match word.link {
                WordLink::None => writeln!(out, "  .word 0x{:08x}", word.value).unwrap(),
                WordLink::Type(i) => writeln!(out, "  .type {}", segment.types[i].name).unwrap(),
                WordLink::Pointer(label) => {
                    let (label_id, byte_offset) = label_id_offset(label.0, label.1);
                    if byte_offset == 0 {
                        writeln!(out, ".word L{}", label_id).unwrap();
                    } else {
                        writeln!(out, ".word (L{} + {})", label_id, byte_offset).unwrap();
                    }
                }
                WordLink::Lo16Pointer(label) => {
                    let (label_id, byte_offset) = label_id_offset(label.0, label.1);
                    assert_eq!(byte_offset, 0);
                    writeln!(
                        out,
                        "  .half 0x{:04x} .half lo16(L{})",
                        word.value >> 16,
                        label_id
                    )
                    .unwrap();
                }
                WordLink::Hi16Pointer(label) => {
                    let (label_id, byte_offset) = label_id_offset(label.0, label.1);
                    assert_eq!(byte_offset, 0);
                    writeln!(
                        out,
                        "  .half 0x{:04x} .half hi16(L{})",
                        word.value >> 16,
                        label_id
                    )
                    .unwrap();
                }
            }
        }

        writeln!(out, "").unwrap();
    }

    println!("{}", out);
}

fn main() -> ExitCode {
    let mut args = env::args_os();

    args.next();

    let Some(path) = args.next() else {
        eprintln!("usage: objdump <file>");
        return ExitCode::FAILURE;
    };

    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("Failed to read {}: {err}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };

    println!("read {} bytes", bytes.len());

    let mut input = bytes.as_slice();
    let file = match goalc::ObjectFile::read(&mut input) {
        Ok(header) => header,
        Err(err) => {
            eprintln!("invalid header: {err}");
            return ExitCode::FAILURE;
        }
    };

    objdump(&file);

    ExitCode::SUCCESS
}
