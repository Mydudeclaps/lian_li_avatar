# Renderer patch

`lian-li-linux-avatar.patch` adds the renderer features Patch needs:

- named video/APNG selector variants driven by a small state file;
- logical-screen position commands with smooth two-axis movement;
- premultiplied-alpha crossfades without the opaque-black flash;
- lossless compressed APNG frame storage and variant deduplication;
- compressed full-screen video frames to keep memory bounded;
- relative variant-path rewriting and backward-compatible template schema.

The patch contains ten files and targets
[`sgtaziz/lian-li-linux`](https://github.com/sgtaziz/lian-li-linux) at commit
`d262007c9bfbe87ae7c9d390d68ec74e5deb4d0a`.

```bash
git clone https://github.com/sgtaziz/lian-li-linux
cd lian-li-linux
git checkout d262007c9bfbe87ae7c9d390d68ec74e5deb4d0a
git apply --check /path/to/lian_li_avatar/patches/lian-li-linux-avatar.patch
git apply /path/to/lian_li_avatar/patches/lian-li-linux-avatar.patch
cargo test -p lianli-shared -p lianli-media
```

The upstream files represented in this patch remain covered by the included
MIT license in `UPSTREAM_LICENSE`. Device, daemon, USB, coolant-controller,
udev, and local probe changes are deliberately not bundled.
