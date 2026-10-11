# skeletons-engine

The engine behind [skeletons](https://github.com/hexlace/skeletons#readme): the
code that reads the skeletons a repository wears, compares the repository's
files against them (`check`), writes the files back (`sync`), starts a
repository wearing a skeleton (`wear`) and stops it (`remove`), together with
the wording each of them answers in.

Without a feature it offers `read` and `survey`, for reading a workspace and
reporting on it as data, with the items they take and return. The `commands`
feature, off by default, adds `task()`, the group of the commands that
`skeletons` hands on, together with the command-line framework, the network
client and the manifest editor they are built on.

This crate exists for `skeletons` and `skeletons-library`, and is published
only because they depend on it. It makes no stability promise: anything in it
may change in any release. Depend on one of those two instead.
