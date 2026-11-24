# cw-fs

File access for a program's private data directory: durable writes, appends, bounded
reads, owner-only checks, and one advisory lock with a deadline. Mechanism only: it knows
no record and no file name.

| Call | What it does |
|---|---|
| `write_atomic(path, bytes)` | Replace: temporary file in the same directory, sync, rename, sync the directory |
| `create_new(path, bytes)` | The same, but never replaces anything |
| `append(path, line)` | One `write` call and a sync; a short write is an error and may leave a prefix; creates the file `0600` when absent |
| `read(path, Cap)` | The whole file, refused past the cap; never a link, FIFO or device |
| `read_tail(path, Cap)` | The last `Cap` bytes, however large the file; may start mid-line; opened and checked as `read` |
| `list(dir)` | Sorted names; an absent directory is empty; `.DS_Store` and `._*` left out |
| `remove(path)` | Remove a regular file and sync the directory |
| `create_private_dir(path)` | `0700`, each new component |
| `inspect_file(path)` / `inspect_dir(path)` | `false` when absent; refused when unsafe |
| `Lock::acquire(path, label, wait)` | Exclusive advisory lock, waiting up to `wait` |

## Guarantees

- **Durable.** A crash leaves the old file or the new one, never a torn one. Once a write
  returns, a crash cannot bring the old file back only when the directory sync succeeded.
  On Linux and macOS it runs, and any failure is an error, except that a filesystem that
  cannot sync a directory (`EINVAL`, `ENOTSUP`) is tolerated and the sync is skipped. On
  native Windows the directory sync is a no-op. In both of those cases the guarantee is the
  filesystem's own.
- **Owner-only.** Everything is created `0700` or `0600`. `inspect_file`, `inspect_dir`,
  `read`, `append` and `Lock::acquire` refuse state owned by another user, or with any group
  or other permission bit, and the error names the `chmod` that fixes it, with the path
  quoted for the shell. A symbolic link to a directory is refused like any other link.
- **A file opened by name is a regular file.** `lstat`, then `O_NOFOLLOW | O_NONBLOCK`,
  then a check of the descriptor: a link, FIFO, device or directory is refused without
  blocking.

## The lock

An OS advisory lock (`std::fs::File::try_lock`: `flock` on Unix, `LockFileEx` on Windows)
on a file that is never deleted, so the kernel releases it when its holder dies.

- A deadline is required. Polling starts at 1 ms and doubles to 20 ms, and no attempt is
  started after the deadline. The deadline bounds the waiting, not the instant the lock is
  taken: an attempt that starts just before it may finish a few microseconds after.
- The holder records one JSON line (pid, time, label; at most 4096 bytes, so a long label
  is cut) after acquiring and clears it on release. A waiter that times out gets `Error::Locked` naming the holder only while that
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

Supported: Linux, macOS, and Windows through WSL2 (which is Linux). Native Windows is
compile-checked only, and has these limits:

| Matter | Linux and macOS | Native Windows |
|---|---|---|
| Symbolic link at the path | Refused | Refused |
| Owner, mode and hard-link checks | Run | Skipped |
| Directory flush after a rename | Run (`EINVAL`/`ENOTSUP` tolerated) | No-op |
| Holder named in a lock timeout | When the recorded process is alive | Never, this process included (the record cannot be read through the holder's lock) |
| Same thread re-acquiring a lock | Refused for any path spelling | Refused only for the same spelling |

Whether a recorded holder is alive: Linux checks the pid and the process's start time, so an
exited process, a zombie, a reused pid, or a record with no start time (an older build's, or
one written when `/proc` could not be read) is not named. macOS checks only that the pid exists,
so a zombie or a reused pid may be named.

## Example

```sh
cargo run -p cw-fs --example private_store
```

## License

MIT.
