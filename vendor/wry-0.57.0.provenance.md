# wry 0.57.0 with close requests and failed-construction ownership

Atlas must persist a page-requested tab close before releasing its native
web view. The owner approved the concrete Windows callback and its narrow
vendor exception on 2026-10-10, as described in
`docs/page-close-event-review.md`. This is the source import record;
compilation, Atlas event routing tests and native callback delivery are
separate checks and were not run during this import.

The owner separately approved the frozen failed-construction patch, including
its documented limitations, on 2026-10-10. It is now applied exactly as reviewed
in `docs/wry-failed-construction-patch-review.md`. This record distinguishes
both patches from their still separate runtime validation; it does not diagnose
the original breakpoint or authorize a commit, merge or public submission.

## Source and scope

The baseline is the complete published
[wry 0.57.0 registry archive](https://static.crates.io/crates/wry/wry-0.57.0.crate),
read from the existing Cargo cache without downloading or executing it.
Its SHA-256 is `a819957a01b3119af85e638a38d242af76dbc87d130dca67bfd0441072e21ff0`, matching the registry source recorded in
Atlas's lockfile before the path patch. The packaged `.cargo_vcs_info.json`
identifies upstream revision `792d0359ba6501a4fc360ece17de2ae42329a47c` with
`dirty: true`. The published archive, not an assumed clean checkout of that
revision, is therefore the authoritative byte baseline.

All 84 published files are retained, including examples, inactive platform
backends, package metadata, the package's own Cargo.lock and build script.
They do not add platform support to Atlas. The archive contains
873,413 bytes; the patched directory contains
880,146 bytes in 85 files. The earlier close-only package was 874,270 bytes
in 84 files. Two original files differ and one private module is added:

- `src/lib.rs` adds a Windows-only optional reference-counted callback,
  defaults it to absent, and exposes its builder method.
- `src/webview2/mod.rs` passes that callback into handler installation and
  calls it instead of `DestroyWindow` when present. The absent-callback
  branch retains the published default behavior. Closing must be deferred
  until the WebView2 callback returns.
- The additional approved change in `src/webview2/mod.rs` retains temporary
  owners through controller creation, configuration and final sizing, and
  delays the non-child parent subclass until handoff. New private
  `src/webview2/construction.rs` owns failure cleanup and its structural
  diagnostics. Its limitations and added internal errors are documented in
  the failed-construction review; no public API is added by this second patch.

The patch does not alter wry's manifests, build script, features, dependencies,
page identity, IPC, initialization scripts, request handling or COM setup.
The other 82 files compare byte-for-byte equal with the verified archive.
The scoped `.gitattributes` rules disable text conversion for this package
and the patch artifact, preserving these hashes across Git checkouts.

## Licence and approval boundary

The unchanged package manifest declares `Apache-2.0 OR MIT`. `LICENSE-APACHE`,
`LICENSE-MIT` and `LICENSE.spdx` are retained unchanged. The SPDX metadata's
`DataLicense: CC0-1.0` applies to that licence record, not to a replacement
licence for wry's source. No new code licence or dependency version is introduced.

The narrow owner approval permits unchanged upstream comments and existing
lint annotations only inside `vendor/wry-0.57.0`. It does not permit new Atlas
lint suppressions, further dependency patches, commits, merges, publication or
an upstream message. The separately approved failed-construction patch retains
these restrictions. Frizbee and the unresolved COM accounting question remain
outside both wry patches.

## Reproducible local patch

`vendor/wry-0.57.0.window-close-requested.patch` is the complete two-file
unified diff, with paths relative to the published wry package. Its SHA-256 is
`dcca8a8f0705cfc8657b71891574596c037adf96efc7247a1a4105b0ce0617cf`. It is prepared locally under D12 and has not been submitted
upstream. Applying it to the checksum-verified archive first produces the
close-only `src/webview2/mod.rs` hash
`2edff46017af862fe60e2ef685af8093f1201cf4f94f2e374a788db98e7f8f45`
and the `src/lib.rs` final hash below.

Then apply `vendor/wry-0.57.0.failed-construction.patch`, SHA-256
`a87ca514149545576a25a39a390a444234d6db9159bcde751e7806aafc959042`,
to produce the current final hashes. This is the exact owner-approved frozen
diff, retained byte-for-byte from the proposal. Reversing these patches in the
opposite order reproduces the published baseline. Neither was sent upstream.
No manifest, feature, pin or lockfile change accompanies the second patch.

Independent application audits compared all 84 close-only package files:
only `src/webview2/mod.rs` changed, `src/webview2/construction.rs` was added,
and the remaining 83 original files are byte-identical. Evidence is in
`target/wry-failed-construction-proposal/applied-source-audit.md` and
`target/wry-failed-construction-proposal/applied-independent-review.md`.
All compilation and native validation is serialized by the integration chat;
source identity alone does not establish runtime acceptance.

Atlas's workspace excludes this vendor directory and selects it with a
`[patch.crates-io]` path override. The exact `=0.57.0` dependency requirement
and existing features remain unchanged. Root owns the subsequent Cargo
resolution: verify that only wry's source/checksum fields change, with no
other package versions or dependency edges shifted. No Cargo command or
root lockfile edit was performed by this import.

## Complete baseline and final inventory

Each SHA-256 covers the exact file bytes. Equal columns prove retained files;
the two changed original rows and one added row are the approved source patches. The provenance and
patch artifact live beside the vendor directory and are not package files.

| File | Bytes, baseline / final | Baseline SHA-256 | Final SHA-256 |
| --- | ---: | --- | --- |
| `.cargo/audit.toml` | 323 / 323 | `26f5e627f768cce1687f4460d56a18b4725817d5ae392dfd154541ef39c40e3b` | `26f5e627f768cce1687f4460d56a18b4725817d5ae392dfd154541ef39c40e3b` |
| `.cargo/config.toml` | 132 / 132 | `0c9559057a73238c99857c7bc3b58243306ebcfe3dcc4c90027da2502186eb11` | `0c9559057a73238c99857c7bc3b58243306ebcfe3dcc4c90027da2502186eb11` |
| `.cargo_vcs_info.json` | 113 / 113 | `3872a04ca66822e4380ce8990a6083485da861207577686ba9fae1da800ae577` | `3872a04ca66822e4380ce8990a6083485da861207577686ba9fae1da800ae577` |
| `.gitignore` | 198 / 198 | `dadf1c3dd73d49c9bce82d54a2a141e2712ab11df1d97b0d6ae99bb591ff440c` | `dadf1c3dd73d49c9bce82d54a2a141e2712ab11df1d97b0d6ae99bb591ff440c` |
| `.license_template` | 151 / 151 | `9aca48869404894538e54a881bcc2ece2142a709b665ced1fd2d284ae7c44595` | `9aca48869404894538e54a881bcc2ece2142a709b665ced1fd2d284ae7c44595` |
| `CHANGELOG.md` | 154537 / 154537 | `0a90dc50dd6a32964a8a60f1693b0ca294285b99966d3627c56defe154592401` | `0a90dc50dd6a32964a8a60f1693b0ca294285b99966d3627c56defe154592401` |
| `Cargo.lock` | 109232 / 109232 | `3a58ebd77fd1bbdd00e8a1df35e972e0fb89682cae57b791071fe9ceb0380988` | `3a58ebd77fd1bbdd00e8a1df35e972e0fb89682cae57b791071fe9ceb0380988` |
| `Cargo.toml` | 9721 / 9721 | `e67fe2036ffcbe811b4c682181efc8a8005ee51ab5b5f254d51639d00a8ee4fb` | `e67fe2036ffcbe811b4c682181efc8a8005ee51ab5b5f254d51639d00a8ee4fb` |
| `Cargo.toml.orig` | 5594 / 5594 | `46225e1e42004bd3ab17155d4abb236c94d9feed78b39fdaf18600db695d993a` | `46225e1e42004bd3ab17155d4abb236c94d9feed78b39fdaf18600db695d993a` |
| `LICENSE-APACHE` | 10847 / 10847 | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| `LICENSE-MIT` | 1121 / 1121 | `ef88893533af22ffbb68ceef0724ba1389518df26165a34f3c99ad30b884df4f` | `ef88893533af22ffbb68ceef0724ba1389518df26165a34f3c99ad30b884df4f` |
| `LICENSE.spdx` | 833 / 833 | `26235c41e314fb0da35b33b2bb5bcbc4dd523839f8eeca52efcd43586f441fb5` | `26235c41e314fb0da35b33b2bb5bcbc4dd523839f8eeca52efcd43586f441fb5` |
| `MOBILE.md` | 10747 / 10747 | `7e4311eb2128c6605b51b3f4c3578f8e49c2595ada41986a8bc09bf20950fd08` | `7e4311eb2128c6605b51b3f4c3578f8e49c2595ada41986a8bc09bf20950fd08` |
| `README.md` | 10518 / 10518 | `cfa18c4d0e5a72d02b71e28c381de76d870d2adac8acf7b077071a5431ee536d` | `cfa18c4d0e5a72d02b71e28c381de76d870d2adac8acf7b077071a5431ee536d` |
| `SECURITY.md` | 664 / 664 | `84ed305e72c0348770feb82dacd98478ad7fc024eedd29e9e9794025f8e04bfe` | `84ed305e72c0348770feb82dacd98478ad7fc024eedd29e9e9794025f8e04bfe` |
| `build.rs` | 4429 / 4429 | `689c618b57d95f2d9e4578744c8126615d93c5624d9b3747af20650e577ccb1b` | `689c618b57d95f2d9e4578744c8126615d93c5624d9b3747af20650e577ccb1b` |
| `examples/async_custom_protocol.rs` | 2790 / 2790 | `7fcc3cb22197f41da43b18550a705690cac5fa4f07942f4cb36dcd279a7903b8` | `7fcc3cb22197f41da43b18550a705690cac5fa4f07942f4cb36dcd279a7903b8` |
| `examples/cookies.rs` | 1842 / 1842 | `ef2f4464f9fa5c536e17f3f9ff46f557e99ad6e3aa5e46e37d53ae96f50a412f` | `ef2f4464f9fa5c536e17f3f9ff46f557e99ad6e3aa5e46e37d53ae96f50a412f` |
| `examples/custom_protocol.rs` | 2723 / 2723 | `a36b0a6f50f23a09e3a452f21688ea6372f525f2bcf90a4228aea369a429613d` | `a36b0a6f50f23a09e3a452f21688ea6372f525f2bcf90a4228aea369a429613d` |
| `examples/custom_protocol/index.html` | 795 / 795 | `86a935667a25d8cc7d4ccbff68b6067c702efc6bc435dc62ce46a498a87678e7` | `86a935667a25d8cc7d4ccbff68b6067c702efc6bc435dc62ce46a498a87678e7` |
| `examples/custom_protocol/script.js` | 843 / 843 | `6e08b96e8e1e0a3acb924a4ea63d45f45544ed82b253b8b9e33aace1e323656e` | `6e08b96e8e1e0a3acb924a4ea63d45f45544ed82b253b8b9e33aace1e323656e` |
| `examples/custom_protocol/subpage.html` | 501 / 501 | `39a21e00664d114f7d6a0c841be473deb0c5e142712decb711377e86c48afd51` | `39a21e00664d114f7d6a0c841be473deb0c5e142712decb711377e86c48afd51` |
| `examples/custom_protocol/wasm.wasm` | 92 / 92 | `2cb1a33779de77d7cc016d934f91262b0984ec695e1f8dda9ba3c33725f5ef1b` | `2cb1a33779de77d7cc016d934f91262b0984ec695e1f8dda9ba3c33725f5ef1b` |
| `examples/custom_titlebar.rs` | 9172 / 9172 | `40b62be794d0e2fd2d5ae548a1dfdc94f48290260c8f6d4fdab8920420614107` | `40b62be794d0e2fd2d5ae548a1dfdc94f48290260c8f6d4fdab8920420614107` |
| `examples/gtk_multiwebview.rs` | 3931 / 3931 | `38f818480c5710420d3a0f83ba575d2c3ac9b71255aa8475337c04696ab4a813` | `38f818480c5710420d3a0f83ba575d2c3ac9b71255aa8475337c04696ab4a813` |
| `examples/gtk_opengl.rs` | 6645 / 6645 | `c9f9e1319709264c90a98bd2238294b743537857dc2cb4d6df22c92c59aa30d5` | `c9f9e1319709264c90a98bd2238294b743537857dc2cb4d6df22c92c59aa30d5` |
| `examples/multiwebview.rs` | 4808 / 4808 | `435326c3fc182084f774bd0924d7eadffe2ead8fad9c9c2656450ea7978710ca` | `435326c3fc182084f774bd0924d7eadffe2ead8fad9c9c2656450ea7978710ca` |
| `examples/multiwindow.rs` | 3478 / 3478 | `86a8dbe10e1913c5b67b0667998ca005886323cce13edc3ac65261b5c5d87723` | `86a8dbe10e1913c5b67b0667998ca005886323cce13edc3ac65261b5c5d87723` |
| `examples/permission_handler.rs` | 1804 / 1804 | `4ce919e92462170937ef0b1f20f18e7e91f68c85160aad021b1449e00f19fc25` | `4ce919e92462170937ef0b1f20f18e7e91f68c85160aad021b1449e00f19fc25` |
| `examples/reparent.rs` | 2852 / 2852 | `9ad2e4b46eeab665bee9cfb19f8407f5f5fd7cce34cb010d56e043ff36c6a534` | `9ad2e4b46eeab665bee9cfb19f8407f5f5fd7cce34cb010d56e043ff36c6a534` |
| `examples/simple.rs` | 1833 / 1833 | `dd060533de836fae76add7185a6c41a7577271aed7cb38e6e55d612512a75c09` | `dd060533de836fae76add7185a6c41a7577271aed7cb38e6e55d612512a75c09` |
| `examples/streaming.rs` | 7900 / 7900 | `ab7e7e1559ebb22d2f40e06e98ccc40adbd209a2281facb36d56646a3bba36c3` | `ab7e7e1559ebb22d2f40e06e98ccc40adbd209a2281facb36d56646a3bba36c3` |
| `examples/streaming/index.html` | 1160 / 1160 | `7eb6977535c62e269b91f85ea6fd5e2ac0da4e8afd33d925e6ffeebe2f1b31d3` | `7eb6977535c62e269b91f85ea6fd5e2ac0da4e8afd33d925e6ffeebe2f1b31d3` |
| `examples/transparent.rs` | 2076 / 2076 | `0f1f428e5ae771725b886108fa595892b5d3303146d761e86c31d700920eee08` | `0f1f428e5ae771725b886108fa595892b5d3303146d761e86c31d700920eee08` |
| `examples/wgpu.rs` | 7878 / 7878 | `90f6b825602370c90ff5781a1e82f6b7a9e2d09e62be0249418f9a98935dbf88` | `90f6b825602370c90ff5781a1e82f6b7a9e2d09e62be0249418f9a98935dbf88` |
| `examples/window_border.rs` | 2838 / 2838 | `fa859ac7bdf738ec5f620c15aa3e3cec07409fcbc9d5ef72fa757635e83c1f53` | `fa859ac7bdf738ec5f620c15aa3e3cec07409fcbc9d5ef72fa757635e83c1f53` |
| `examples/winit.rs` | 2520 / 2520 | `fd1263474c2dd3057beeab2d3e582f6d35df8b049c37655d86a4ddba5c4b94cd` | `fd1263474c2dd3057beeab2d3e582f6d35df8b049c37655d86a4ddba5c4b94cd` |
| `renovate.json` | 177 / 177 | `32cf3c60bb8d28e5bb05d6564f413edcf8440261de9c6d9e2882f948a61a26a2` | `32cf3c60bb8d28e5bb05d6564f413edcf8440261de9c6d9e2882f948a61a26a2` |
| `rustfmt.toml` | 377 / 377 | `355f303583e15d1518e0c89eeaf2a4034bcc7c2a292087dfc17d00dc8173a5fd` | `355f303583e15d1518e0c89eeaf2a4034bcc7c2a292087dfc17d00dc8173a5fd` |
| `src/android/binding.rs` | 16687 / 16687 | `97afdd339a5d6736af1a767201d6ef0cfb7526bc4d5e35577842739447dad587` | `97afdd339a5d6736af1a767201d6ef0cfb7526bc4d5e35577842739447dad587` |
| `src/android/kotlin/Ipc.kt` | 711 / 711 | `80232ae2df8264b7855ef21aafff772d8563aae6b2374c49394421cb085c0b10` | `80232ae2df8264b7855ef21aafff772d8563aae6b2374c49394421cb085c0b10` |
| `src/android/kotlin/Logger.kt` | 1921 / 1921 | `6ef1209ea4a45ef236fd4e2b0d1c2b086bc99ddb31a4e1d9da171f45c06a51fe` | `6ef1209ea4a45ef236fd4e2b0d1c2b086bc99ddb31a4e1d9da171f45c06a51fe` |
| `src/android/kotlin/PermissionHelper.kt` | 4182 / 4182 | `15fbd8ac2606c9f930cfd047a1074cedec286b7257e2a644e5caa45fdcab74e4` | `15fbd8ac2606c9f930cfd047a1074cedec286b7257e2a644e5caa45fdcab74e4` |
| `src/android/kotlin/Rust.kt` | 1867 / 1867 | `ed069b83c5d9acfd6ce241ff25797362ba3d058cf7356a713165d5f5bc506deb` | `ed069b83c5d9acfd6ce241ff25797362ba3d058cf7356a713165d5f5bc506deb` |
| `src/android/kotlin/RustWebChromeClient.kt` | 17024 / 17024 | `5e225fc1a85e769e4a666b7cd47946356e742190060803ecb31a12a68bf1ce06` | `5e225fc1a85e769e4a666b7cd47946356e742190060803ecb31a12a68bf1ce06` |
| `src/android/kotlin/RustWebView.kt` | 2782 / 2782 | `0c4e6e8bde658f0e2ac7563a0cc2c164d0d39af369998ddd7f5c8fd9745b4236` | `0c4e6e8bde658f0e2ac7563a0cc2c164d0d39af369998ddd7f5c8fd9745b4236` |
| `src/android/kotlin/RustWebViewClient.kt` | 3772 / 3772 | `1e5ec43b3ae2315aeaa4d5721435076c3a26b84276ac9e83fe3b331f4277f4f8` | `1e5ec43b3ae2315aeaa4d5721435076c3a26b84276ac9e83fe3b331f4277f4f8` |
| `src/android/kotlin/WryActivity.kt` | 6572 / 6572 | `585a83565bfdf924600d0f0b40eda26bce76d2947653a4fec9a87b09e21dc11d` | `585a83565bfdf924600d0f0b40eda26bce76d2947653a4fec9a87b09e21dc11d` |
| `src/android/kotlin/proguard-wry.pro` | 841 / 841 | `6b10700539fe7f2d20616387a79fc6c1ebfa729691031feb2952f368ade73f51` | `6b10700539fe7f2d20616387a79fc6c1ebfa729691031feb2952f368ade73f51` |
| `src/android/main_pipe.rs` | 19852 / 19852 | `81fa4d9b38df667cc4a84f784ae3a9a990eec8a3bc0029ceb14a483bee982f40` | `81fa4d9b38df667cc4a84f784ae3a9a990eec8a3bc0029ceb14a483bee982f40` |
| `src/android/mod.rs` | 15897 / 15897 | `87136468504e92e0d1011c823ec7b3c9000cff13046b1513ee165d3109727028` | `87136468504e92e0d1011c823ec7b3c9000cff13046b1513ee165d3109727028` |
| `src/custom_protocol_workaround.rs` | 2081 / 2081 | `509ab2ad8c8aadc5746f277d97ae5588bbdd701a6f0bca32f674328d466768b5` | `509ab2ad8c8aadc5746f277d97ae5588bbdd701a6f0bca32f674328d466768b5` |
| `src/error.rs` | 2875 / 2875 | `d25cb6ee2aeb38fce037e02b1ab410aacfe4cd087ba58849cd109ae4fb838bba` | `d25cb6ee2aeb38fce037e02b1ab410aacfe4cd087ba58849cd109ae4fb838bba` |
| `src/inject_initialization_scripts.rs` | 7009 / 7009 | `eff563eee5ff7b1f57fbf84268f8bc4a5853391ab64ed200a9ad60ca537b2f4b` | `eff563eee5ff7b1f57fbf84268f8bc4a5853391ab64ed200a9ad60ca537b2f4b` |
| `src/lib.rs` | 102979 / 103522 | `f5c9e28fbb1b6a4ddfed7ddface796a9e91689e8663457ed69b651ad00149ba7` | `6044247c8411139d68d7c778fc8971084c89fd544d65219b01abdbde7cd3dd93` |
| `src/permissions.rs` | 5892 / 5892 | `4d933418850508a0423cdbe45fd091d0f8eeb3ccab06c9ee70dbf354032552ea` | `4d933418850508a0423cdbe45fd091d0f8eeb3ccab06c9ee70dbf354032552ea` |
| `src/proxy.rs` | 403 / 403 | `c72606d68787704fa91088d81f6b495fc05ce92ba77845339819b4d0c004a8f0` | `c72606d68787704fa91088d81f6b495fc05ce92ba77845339819b4d0c004a8f0` |
| `src/util.rs` | 237 / 237 | `5f1b64eab6c81dea1c6ce7f955d2a7888b368ef9fcbe574bb1187e4046135e98` | `5f1b64eab6c81dea1c6ce7f955d2a7888b368ef9fcbe574bb1187e4046135e98` |
| `src/web_context.rs` | 3591 / 3591 | `dd9912d669f3a5de10aeda7b8e1361751a1137a602385d31964439f101773aac` | `dd9912d669f3a5de10aeda7b8e1361751a1137a602385d31964439f101773aac` |
| `src/webkitgtk/drag_drop.rs` | 3602 / 3602 | `e3e52b6c32543c7e415244766a4a9da1decb028b50ccaaf3d981f8566acc8ed8` | `e3e52b6c32543c7e415244766a4a9da1decb028b50ccaaf3d981f8566acc8ed8` |
| `src/webkitgtk/mod.rs` | 40597 / 40597 | `20a4064b352747b46d7e15ae6962922dfa85017660e109e87e255219c6ee2017` | `20a4064b352747b46d7e15ae6962922dfa85017660e109e87e255219c6ee2017` |
| `src/webkitgtk/synthetic_mouse_events.rs` | 5046 / 5046 | `e534ed5d79d3b0e79a6341cefa57bb20438ec34db32983522bf6816bdbdab2b0` | `e534ed5d79d3b0e79a6341cefa57bb20438ec34db32983522bf6816bdbdab2b0` |
| `src/webkitgtk/web_context.rs` | 13946 / 13946 | `ef43edb59e19e60743c7858be846cac23ce8e7a188f77b12ac7e0a82a907628f` | `ef43edb59e19e60743c7858be846cac23ce8e7a188f77b12ac7e0a82a907628f` |
| `src/webview2/drag_drop.rs` | 7261 / 7261 | `ec03770d82dbbf47cbc3932a1b709bd8aed5a0914e20b6afdcc6629fed99c7ee` | `ec03770d82dbbf47cbc3932a1b709bd8aed5a0914e20b6afdcc6629fed99c7ee` |
| `src/webview2/mod.rs` | 59539 / 60929 | `4800257d9d251c59b5aa464814ade5f8f041a854f0e4498afbdb036eba5307b8` | `23e0ade313f6fb888854e5f3aca0dfe46807b77a39f41660795520c6a429a512` |
| `src/webview2/construction.rs` | absent / 4800 | absent | `c446c9c14056ae838eef05d26cd4e0504d0ed5c228ca10c3983730c377f8aa76` |
| `src/webview2/util.rs` | 3224 / 3224 | `068edfbcda4060a789f9e1f6dea83ba748e560a33c17179aa7839425f52ade39` | `068edfbcda4060a789f9e1f6dea83ba748e560a33c17179aa7839425f52ade39` |
| `src/wkwebview/class/document_title_changed_observer.rs` | 2400 / 2400 | `7e09f34f232f7838ecfa11c0d75d0361d32df91aa9ce9e041915e316f0784af8` | `7e09f34f232f7838ecfa11c0d75d0361d32df91aa9ce9e041915e316f0784af8` |
| `src/wkwebview/class/mod.rs` | 391 / 391 | `f20048d06e0098d25eee0c549eb7fe26b20d041ada2fe1daf7771125fd44a7b8` | `f20048d06e0098d25eee0c549eb7fe26b20d041ada2fe1daf7771125fd44a7b8` |
| `src/wkwebview/class/url_scheme_handler.rs` | 12090 / 12090 | `09a55a1804dbf40c0bc2ed2e19a53199136c53cc143c393bb6d0123736aa2af4` | `09a55a1804dbf40c0bc2ed2e19a53199136c53cc143c393bb6d0123736aa2af4` |
| `src/wkwebview/class/wry_download_delegate.rs` | 2354 / 2354 | `d52460df67678eccb4c5d409c9d24a81d5c63ff04eee6a76e5558c9ccd341756` | `d52460df67678eccb4c5d409c9d24a81d5c63ff04eee6a76e5558c9ccd341756` |
| `src/wkwebview/class/wry_navigation_delegate.rs` | 5208 / 5208 | `e858a92f23ddb5d67d117ed5c64c31782a49e3d6234a96c364e0c06ab5e16cb6` | `e858a92f23ddb5d67d117ed5c64c31782a49e3d6234a96c364e0c06ab5e16cb6` |
| `src/wkwebview/class/wry_web_view.rs` | 4783 / 4783 | `692b4420ace30fafece0707533d68b40b82c20ee7f5843eb17e04a7c6fcb4656` | `692b4420ace30fafece0707533d68b40b82c20ee7f5843eb17e04a7c6fcb4656` |
| `src/wkwebview/class/wry_web_view_delegate.rs` | 3209 / 3209 | `97559a443b53682d98858effed722774b309b7d72aac1aacc573f1d989fae093` | `97559a443b53682d98858effed722774b309b7d72aac1aacc573f1d989fae093` |
| `src/wkwebview/class/wry_web_view_parent.rs` | 3644 / 3644 | `c405e3ea1d2e062a43847c6d7c91be6da36026f3230bf47f2661bddc6852012a` | `c405e3ea1d2e062a43847c6d7c91be6da36026f3230bf47f2661bddc6852012a` |
| `src/wkwebview/class/wry_web_view_ui_delegate.rs` | 11368 / 11368 | `931ad3ca6e303963e4d2c4267e2cd9fb94a0eef1ac8af8e6602b30cdcc324cc3` | `931ad3ca6e303963e4d2c4267e2cd9fb94a0eef1ac8af8e6602b30cdcc324cc3` |
| `src/wkwebview/download.rs` | 3987 / 3987 | `3b67113c3d13a2082992d284a1e995e07c06a9b892b3ed2dae9b7370daa0f6c0` | `3b67113c3d13a2082992d284a1e995e07c06a9b892b3ed2dae9b7370daa0f6c0` |
| `src/wkwebview/drag_drop.rs` | 3658 / 3658 | `10bf1524a41d586810cae18d318629da3768e69a1b0ab59ad04dfd69840d83c7` | `10bf1524a41d586810cae18d318629da3768e69a1b0ab59ad04dfd69840d83c7` |
| `src/wkwebview/ios/WKWebView.rs` | 20585 / 20585 | `4ff8588c4d8773c244c228e5aa1cce3af8d62e4de1fb08cb80dfb31ae9a2dbb0` | `4ff8588c4d8773c244c228e5aa1cce3af8d62e4de1fb08cb80dfb31ae9a2dbb0` |
| `src/wkwebview/ios/mod.rs` | 19 / 19 | `03823ce7c572f1ed345ae4ef6ec63e8ce4df1f0a7d3ad0efebb36dd6a7c18dd1` | `03823ce7c572f1ed345ae4ef6ec63e8ce4df1f0a7d3ad0efebb36dd6a7c18dd1` |
| `src/wkwebview/mod.rs` | 52193 / 52193 | `a466a6d6a5fa75013876492a2074c3693601bcac63716de0c9cc54bceab5bf6b` | `a466a6d6a5fa75013876492a2074c3693601bcac63716de0c9cc54bceab5bf6b` |
| `src/wkwebview/navigation.rs` | 3357 / 3357 | `1898665e051edf10124178aa596b02dd121b33e1ccf7bb48b54d859d46e73aae` | `1898665e051edf10124178aa596b02dd121b33e1ccf7bb48b54d859d46e73aae` |
| `src/wkwebview/proxy.rs` | 1474 / 1474 | `f7ee208069b3d4d045540c46b842c71f0fa2bba905b20463e67679471f58ad20` | `f7ee208069b3d4d045540c46b842c71f0fa2bba905b20463e67679471f58ad20` |
| `src/wkwebview/synthetic_mouse_events.rs` | 3673 / 3673 | `729487a4009431c29633c0ada6d78c513a18af148c118aefa9b0279c322c7cb2` | `729487a4009431c29633c0ada6d78c513a18af148c118aefa9b0279c322c7cb2` |
| `src/wkwebview/util.rs` | 435 / 435 | `c7be7b4421d3d4ba06a61d39f4efc3ada0eb3e2d691078aa842c3ae76f95e755` | `c7be7b4421d3d4ba06a61d39f4efc3ada0eb3e2d691078aa842c3ae76f95e755` |
