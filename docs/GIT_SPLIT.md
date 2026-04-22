# Serpantoxide Git Split Notes

`Serpantoxide` currently lives as the `Serpantoxide/` subtree inside the parent `pentestagent` repository.

To publish it as a standalone repository while preserving history, split from the parent repo root:

```bash
git subtree split --prefix=Serpantoxide -b serpantoxide-v0.1b
git tag -a v0.1b <split-commit> -m "Serpantoxide v0.1b"
```

After adding a new remote for the standalone repository, push the split branch and tag:

```bash
git push <new-remote> serpantoxide-v0.1b:master
git push <new-remote> v0.1b
```

The release prep for `v0.1b` assumes:

- local scan artifacts are excluded from version control,
- Serpantoxide-specific source, docs, scripts, and skills are committed in the parent repo first,
- the split tag is created from the subtree split commit, not from the parent monorepo commit.
