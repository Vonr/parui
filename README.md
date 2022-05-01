# parui
[![Crates.io](https://img.shields.io/crates/v/parui)](https://crates.io/crates/parui)

### Simple TUI frontend for [paru](https://github.com/morganamilo/paru).

### Images
![parui searching for paru](images/1.png)

### Keybinds

parui adopts vim-like keybinds.

| Key                | Mode   | Action                  |
|--------------------|--------|-------------------------|
| \<Escape\>         | Insert | Enter Select Mode       |
| \<Return\>         | Insert | Search for query        |
| \<C-w\>            | Insert | Removes previous word   |
| \<C-c\>            | Both   | Exits parui             |
| i                  | Select | Enter Insert Mode       |
| \<Return\>         | Select | Selected package info   |
| h, \<Left\>        | Select | Moves one page back     |
| j, \<Down\>        | Select | Moves one row down      |
| k, \<Up\>          | Select | Moves one row up        |
| l, \<Right\>       | Select | Moves one page forwards |
| <C-j>, \<C-Down\>  | Select | Moves info one row down |
| <C-k>, \<C-Up\>    | Select | Moves info one row up   |
| q                  | Select | Exits parui             |
