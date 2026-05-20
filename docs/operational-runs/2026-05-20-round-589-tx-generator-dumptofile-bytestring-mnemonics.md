---
title: "Round 589 tx-generator DumpToFile ByteString mnemonic escapes"
parent: Reference
---

# Round 589 tx-generator DumpToFile ByteString mnemonic escapes

Date: 2026-05-20

## Scope

This round closes the documented byte-parity gap from R572:
`show_haskell_bytestring` now emits the full GHC `showLitChar` +
`showLitString` escape table, so ByteString Show output is
byte-equivalent to upstream `Data.ByteString.unpackChars +
showLitString` for every byte value (0x00-0xFF).

## Upstream references

- GHC `Text.Show.showLitChar` (`base/GHC/Show.hs`): the canonical
  `Char` Show escape table. Defines `\a`/`\b`/`\f`/`\n`/`\r`/`\t`/`\v`
  short aliases, the `\SO` H-lookahead disambiguation, the
  multi-letter mnemonic table (`NUL`...`US`), `\DEL`, and the `\NNN`
  decimal escape with `\&` separator before following digits.

## Changes

- Added the 32-entry `HASKELL_ASCII_TAB` constant (NUL/SOH/STX/ETX/
  EOT/ENQ/ACK/BEL/BS/HT/LF/VT/FF/CR/SO/SI/DLE/DC1/DC2/DC3/DC4/NAK/
  SYN/ETB/CAN/EM/SUB/ESC/FS/GS/RS/US) indexed by byte value 0x00-0x1F.
- Rewrote `show_haskell_bytestring` to match GHC's `showLitChar`
  table exactly:
  - `"` → `\"`
  - `\` → `\\`
  - 0x07-0x0D → short-form aliases `\a`/`\b`/`\t`/`\n`/`\v`/`\f`/`\r`
  - 0x0E → `\SO` with `\&` separator inserted before a following
    `H` so `\SOH` (Start Of Heading) and `\SO`+`H` stay
    distinguishable
  - 0x00-0x06 + 0x0F-0x1F → `\<name>` via `HASKELL_ASCII_TAB`
  - 0x7F → `\DEL`
  - 0x20-0x7E (except `"` and `\`) → inline
  - 0x80-0xFF → `\NNN` decimal escape with `\&` separator before a
    following ASCII digit (unchanged from R572)
- Added `dumptofile_plutus_data_renders_bytes_full_mnemonic_escapes`
  unit test exercising every escape class: short forms (0x07/0x08/
  0x0B/0x0C), multi-letter mnemonics (0x00-0x03), SO with and
  without H lookahead (0x0E + 'H', 0x0E + 'I'), 0x0F+0x1F
  mnemonics, and 0x7F DEL.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (55 tests, +1
  from R588)
- `cargo test -p yggdrasil-tx-generator` (238 lib tests + 5
  CLI/golden, +1 from R588 baseline)

## Remaining

- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets (needs Byron AddressInfo packing port).
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
