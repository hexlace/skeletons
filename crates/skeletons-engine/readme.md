# skeletons-engine

The engine behind [skeletons](https://github.com/hexlace/skeletons#readme): the
code that reads the skeletons a repository wears, compares the repository's
files against them (`check`), writes the files back (`sync`) and starts a
repository wearing a skeleton (`wear`), together with the wording each of them
answers in.

It offers `task()`, the group of the three commands that `skeletons` hands on,
and, for reading a workspace and reporting on it as data, `read` and `survey`
with the items they take and return.

This crate exists for `skeletons` and `skeletons-library`, and is published
only because they depend on it. It makes no stability promise: anything in it
may change in any release. Depend on one of those two instead.
