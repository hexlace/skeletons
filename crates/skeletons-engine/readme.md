# skeletons-engine

The engine behind [skeletons](https://github.com/hexlace/skeletons#readme): the
code that reads the skeletons a repository wears, compares the repository's
files against them (`check`), writes the files back (`sync`), starts a
repository wearing a skeleton (`wear`) and stops it (`remove`), together with
the wording each of them answers in.

Without a feature it offers `read`, `read_without_cargo`,
`read_without_cargo_configuration` and `survey`, for reading a workspace and
reporting on it as data, with the items they take and return. `read` asks
cargo; `read_without_cargo` reads the manifests and `Cargo.lock` as files and
runs no cargo; `read_without_cargo_configuration` is that read with no Cargo
configuration file read. The `commands` feature, off by
default, adds `task()`, the group of the commands that `skeletons` hands on,
together with every dependency only they use: the command-line framework, the
network client and the manifest editor.

This crate exists for `skeletons` and `skeletons-library`, and is published
only because they depend on it. It makes no stability promise: anything in it
may change in any release. Depend on one of those two instead.
