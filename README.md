# Mercurium
Named after the messenger of the gods Mercurius, this is my attempt at a simple package manager.  
**This is not intended to be used! I just created this for fun!**

## Features
- Support for custom packages using `TOML`
- Automatic building from source
- Dependency resolving
- SHA512 checksum checking
- Autocompletion using `clap-complete`

## To-do
- [ ] Decompressing `.tar.xz` and `.zip`
- [ ] Build dependencies
- [ ] Uninstall packages (keep track of installed files, maybe using `fakeroot`?)
- [ ] Better "UX"
    - [ ] Check if payload is empty and stop
    - [ ] Better messages / errors
    - [x] Download progress bars
- [x] Multithreaded downloading
- [ ] More tests
- [ ] Upgrade packages

## Licenses
For all licenses, look into `license.html`.  
This file was automatically created using [cargo-about](https://github.com/EmbarkStudios/cargo-about) (Embark Studios).