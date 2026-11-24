# cw-fs

File access for a program's private data directory: durable writes, appends, bounded
reads, owner-only checks, and one advisory lock with a deadline. Mechanism only: it knows
no record and no file name.

| Call | What it does |
|---|---|
| `write_atomic(path, bytes)` | Replace: temporary file in the same directory, sync, rename, sync the directory |
| `create_new(path, bytes)` | The same, but never replaces anything |
| `append(path, line)` | One write and a sync; creates the file `0600` when absent |
| `read(path, Cap)` | The whole file, refused past the cap; never a link, FIFO or device |
| `list(dir)` | Sorted names; an absent directory is empty; `.DS_Store` and `._*` left out |
| `remove(path)` | Remove a regular file and sync the directory |
| `create_private_dir(path)` | `0700`, each new component |
| `inspect_file(path)` / `inspect_dir(path)` | `false` when absent; refused when unsafe |
| `Lock::acquire(path, label, wait)` | Exclusive advisory lock, waiting up to `wait` |

## Guarantees

- **Durable.** A crash leaves the old file or the new one, never a torn one. A filesystem
  that cannot sync a directory (`EINVAL`, `ENOTSUP`) is tolerated; any other failure is an
  error.
- **Owner-only.** Everything is created `0700` or `0600`. On load, state owned by another
  user, or with any group or other permission bit, is refused, and the error names the
  `chmod` that fixes it.
- **A file opened by name is a regular file.** `lstat`, then `O_NOFOLLOW | O_NONBLOCK`,
  then a check of the descriptor: a link, FIFO, device or directory is refused without
  blocking.

## The lock

An OS advisory lock (`std::fs::File::try_lock`: `flock` on Unix, `LockFileEx` on Windows)
on a file that is never deleted, so the kernel releases it when its holder dies.

- A deadline is required. Polling starts at 1 ms and doubles to 20 ms.
- The holder records one JSON line (pid, time, label) after acquiring and clears it on
  release. A waiter that times out gets `Error::Locked` naming the holder only while that
  process is alive; nothing decides ownership from the record.
- Every acquire opens the file again, so a second thread of one process waits like another
  process. The same thread asking for a lock it holds gets `Error::Reentrant` at once.
- The lock file is opened without following a link and refused when it has more than one
  hard link.

## Errors

`cw_fs::Error` has `kind() -> cw_base::Kind` and no code: the unit that wraps it names its
own code (a store wrapping `Locked` might call it `store_locked`). It is not
`#[non_exhaustive]`.

## Platforms

Unix and Windows. On Windows the owner, mode and link checks are skipped; writes and locks
work.

## Example

```sh
cargo run -p cw-fs --example private_store
```

## License

MIT.
