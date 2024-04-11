/// ```text
/// <block>    = <section> <labelled>
/// <section>  = <section-end> <section-start>
///            | <section-start>
///            | <section-end>
/// <labelled> = <label> <real>
/// <real>     = <instruction> | <error> | <bytes>
/// ```
use std::sync::Arc;

use debugvault::Symbol;
use processor_shared::{encode_hex_bytes_truncated, Section};
use tokenizing::{colors, Token, TokenStream};

use crate::Processor;

#[derive(Debug)]
pub enum BlockContent {
    SectionStart {
        section: Section,
    },
    SectionEnd {
        section: Section,
    },
    Label {
        symbol: Arc<Symbol>,
    },
    Instruction {
        inst: Vec<Token>,
        bytes: String,
    },
    Error {
        err: decoder::ErrorKind,
        bytes: String,
    },
    Bytes {
        bytes: Vec<u8>,
    },
}

#[derive(Debug)]
pub struct Block {
    pub addr: usize,
    pub content: BlockContent,
}

impl Block {
    /// Length of block when tokenized.
    pub fn len(&self) -> usize {
        match &self.content {
            BlockContent::SectionStart { .. } => 2,
            BlockContent::SectionEnd { .. } => 2,
            BlockContent::Label { .. } => 2,
            BlockContent::Instruction { .. } => 1,
            BlockContent::Error { .. } => 1,
            BlockContent::Bytes { bytes } => (bytes.len() / 32) + 1,
        }
    }

    pub fn tokenize(&self, stream: &mut TokenStream) {
        match &self.content {
            BlockContent::Label { symbol } => {
                stream.push("\n<", colors::BLUE);
                stream.inner.extend_from_slice(symbol.name());
                stream.push(">", colors::BLUE);
            }
            BlockContent::SectionStart { section } => {
                stream.push("section started", colors::WHITE);
                stream.push_owned(format!(" {} ", section.name), colors::BLUE);
                stream.push("{", colors::GRAY60);
                stream.push_owned(format!("{:?}", section.kind), colors::MAGENTA);
                stream.push("} ", colors::GRAY60);
                stream.push_owned(format!("{:x}", section.start), colors::GREEN);
                stream.push("-", colors::GRAY60);
                stream.push_owned(format!("{:x}", section.end), colors::GREEN);
            }
            BlockContent::SectionEnd { section } => {
                stream.push("section ended", colors::WHITE);
                stream.push_owned(format!(" {} ", section.name), colors::BLUE);
                stream.push("{", colors::GRAY60);
                stream.push_owned(format!("{:?}", section.kind), colors::MAGENTA);
                stream.push("} ", colors::GRAY60);
                stream.push_owned(format!("{:x}", section.start), colors::GREEN);
                stream.push("-", colors::GRAY60);
                stream.push_owned(format!("{:x}", section.end), colors::GREEN);
            }
            BlockContent::Instruction { inst, bytes } => {
                stream.push_owned(format!("{:0>10X}  ", self.addr), colors::GRAY40);
                stream.push_owned(bytes.clone(), colors::GREEN);
                stream.inner.extend_from_slice(&inst);
            }
            BlockContent::Error { err, bytes } => {
                stream.push_owned(format!("{:0>10X}  ", self.addr), colors::GRAY40);
                stream.push_owned(bytes.clone(), colors::GREEN);
                stream.push("<", colors::GRAY40);
                stream.push_owned(format!("{err:?}"), colors::RED);
                stream.push(">", colors::GRAY40);
            }
            BlockContent::Bytes { bytes } => {
                let mut off = 0;
                // Never print more than 100 lines, this is a little scuffed.
                for chunk in bytes.chunks(32).take(100) {
                    stream.push_owned(format!("{:0>10X}  ", self.addr + off), colors::GRAY40);
                    let s = processor_shared::encode_hex_bytes_truncated(chunk, usize::MAX, false);
                    stream.push_owned(s, colors::GREEN);
                    stream.push("\n", colors::WHITE);
                    off += chunk.len();
                }
                // Pop last newline
                stream.inner.pop();
            }
        }
    }
}

impl Processor {
    fn parse_data_or_code(&self, addr: usize) -> Option<Block> {
        let section = self.section_by_addr(addr).expect("Invalid address.");

        if let Some(inst) = self.instruction_by_addr(addr) {
            let width = self.instruction_width(&inst);
            let inst = self.instruction_tokens(&inst, &self.index);
            let bytes = section.bytes_by_addr(addr, width);
            let bytes =
                encode_hex_bytes_truncated(&bytes, self.max_instruction_width * 3 + 1, true);
            return Some(Block {
                addr,
                content: BlockContent::Instruction { inst, bytes },
            });
        }

        if let Some(err) = self.error_by_addr(addr) {
            let bytes = section.bytes_by_addr(addr, err.size());
            let bytes =
                encode_hex_bytes_truncated(&bytes, self.max_instruction_width * 3 + 1, true);
            return Some(Block {
                addr,
                content: BlockContent::Error {
                    err: err.kind,
                    bytes,
                },
            });
        }

        let mut baddr = addr;
        loop {
            if baddr == section.end {
                break;
            }

            if self.instruction_by_addr(baddr).is_some() {
                break;
            }

            if self.error_by_addr(baddr).is_some() {
                break;
            }

            if addr != baddr && self.index.get_func_by_addr(baddr).is_some() {
                break;
            }

            baddr += 1;
        }

        let bytes_len = baddr - addr;
        if bytes_len > 0 {
            let bytes = section.bytes_by_addr(addr, bytes_len).to_vec();
            return Some(Block {
                addr,
                content: BlockContent::Bytes { bytes },
            });
        }

        None
    }

    /// Pars blocks given an address boundary.
    pub fn parse_blocks(&self, addr: usize) -> Vec<Block> {
        let mut blocks = Vec::new();

        let section_start = self.sections().find(|sec| sec.start == addr);
        let section_end = self.sections().find(|sec| sec.end == addr);

        match (section_start, section_end) {
            (Some(start), Some(end)) => {
                blocks.push(Block {
                    addr,
                    content: BlockContent::SectionEnd {
                        section: end.clone(),
                    },
                });
                blocks.push(Block {
                    addr,
                    content: BlockContent::SectionStart {
                        section: start.clone(),
                    },
                })
            }
            (Some(section), None) => blocks.push(Block {
                addr,
                content: BlockContent::SectionStart {
                    section: section.clone(),
                },
            }),
            (None, Some(section)) => blocks.push(Block {
                addr,
                content: BlockContent::SectionEnd {
                    section: section.clone(),
                },
            }),
            (None, None) => {}
        }

        if let Some(real_block) = self.parse_data_or_code(addr) {
            if let Some(symbol) = self.index.get_func_by_addr(addr) {
                blocks.push(Block {
                    addr,
                    content: BlockContent::Label { symbol },
                })
            }

            blocks.push(real_block);
        }

        blocks
    }

    /// Only need to compute the start's of blocks.
    pub fn compute_block_boundaries(&self) -> Vec<usize> {
        let mut boundaries = Vec::new();
        std::thread::scope(|s| {
            let threads: Vec<_> = self
                .sections()
                .map(|section| s.spawn(|| self.compute_section_boundaries(section)))
                .collect();

            for thread in threads {
                boundaries.extend(thread.join().unwrap());
            }
        });

        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries
    }

    fn compute_section_boundaries(&self, section: &Section) -> Vec<usize> {
        let mut boundaries = Vec::new();
        let mut addr = section.addr;

        boundaries.push(section.start);

        loop {
            if addr == section.end {
                break;
            }

            if self.index.get_func_by_addr(addr).is_some() {
                boundaries.push(addr);
            }

            if let Some(inst) = self.instruction_by_addr(addr) {
                boundaries.push(addr);
                addr += self.instruction_width(inst);
                continue;
            }

            if let Some(err) = self.error_by_addr(addr) {
                boundaries.push(addr);
                addr += err.size();
                continue;
            }

            let mut baddr = addr;
            loop {
                if baddr == section.end {
                    break;
                }

                if self.instruction_by_addr(baddr).is_some() {
                    break;
                }

                if self.error_by_addr(baddr).is_some() {
                    break;
                }

                // We found some labelled bytes, so those would have to be in a different block.
                if addr != baddr && self.index.get_func_by_addr(baddr).is_some() {
                    break;
                }

                baddr += 1;
            }

            let bytes_len = baddr - addr;
            if bytes_len > 0 {
                boundaries.push(addr);
                addr = baddr;
            }
        }

        boundaries.push(section.end);
        boundaries
    }
}