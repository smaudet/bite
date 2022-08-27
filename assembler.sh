#!/bin/sh

read -p "Input assembly instruction: " -r asm

cat << EOF | rustc -o target/asm --crate-type lib - 
#![feature(naked_functions)]
#![no_std]

#[no_mangle]
#[naked]
pub unsafe extern "C" fn asm() {
    core::arch::asm!("$asm", options(noreturn));
}
EOF

objdump -M intel \
    --section=.text.asm \
    -D target/asm \
    | grep -E "\s{3}0:" \
    | sed -E "s/   0:\t/\n/;s/\s{13}//"