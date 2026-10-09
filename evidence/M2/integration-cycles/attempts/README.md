# Superseded install-cycle run

`20261009T022915Z-dev` passed: ten install/reinstall/remove cycles per fixture, 15 owned hooks and one owned plugin directory each time, foreign settings kept and the original bytes restored. It ran on the bundle built at `dbbbf0e`. `../20261009T044548Z-dev/` repeats it on the final build, `f4ef5f1`. The installer code (`crates/provider-claude/src/setup/`) did not change between the two.
