# frizbee 0.13.0 with valid ASCII scoring lanes

Atlas must preserve accepted NUL queries without scoring SIMD padding or returning
highlight positions beyond the field. The owner approved the exact three-file
correction in `docs/dev/vendor-reviews/p1-05-frizbee-maintenance-review.md` and the narrow unchanged
upstream comment/lint exception on 2026-10-10. This records the source import;
compilation, regression tests and performance validation are separate checks.

## Source and scope

The baseline is the complete published frizbee 0.13.0 registry archive, read
from `target/dependency-audit/frizbee-0.13.0/source.crate` without downloading
or executing it. Archive SHA-256:
`19435f276a44d90570cb403d22fbe4b20c80eabc134f718d6ec0ae888eb47edc`.
The packaged `.cargo_vcs_info.json` identifies upstream revision
`d608923f526c57c7ac90b79c0cb1b8619b449179`. The published bytes are authoritative.

All 76 packaged files are retained, totalling 721,016 baseline
bytes and 721,432 patched bytes. Exactly three files differ:

- `src/smith_waterman/algo/mod.rs` receives the existing valid-lane helper.
- `src/smith_waterman/algo/unicode.rs` imports that helper instead of defining it.
- `src/smith_waterman/algo/ascii.rs` masks match identities and stored scores
  against actual haystack bytes, calculating the mask once per chunk.

The other 73 files are byte-for-byte identical, including manifests,
licence, packaged lockfile, tests and inactive backends. The MIT licence,
`=0.13.0` pin, `default-features=false`, `std` and `safe_read` are unchanged.
This import adds no Atlas-authored suppression, dependency, feature, benchmark,
input restriction or score/highlight workaround. The over-1024-byte greedy path
is unchanged. Existing upstream comments and lint annotations remain intact.

The adjacent `frizbee-0.13.0.padding.patch` is the complete upstream-ready diff
with package-relative paths. Its SHA-256 is
`7324609c1a7dabbb50b7a40ca61212b92883606b86fa89927d33c67f104218d3`. No upstream message, commit, merge or push is authorized.

## Verification and lockfile expectation

The archive identity and three approved baseline hashes were asserted before
writing. Every output path and byte sequence was then checked against the full
archive inventory, allowing only the three documented source differences. The
root Cargo override uses this local package; the expected root lockfile change
is removal of frizbee's registry source/checksum only, without changing its
version or any other package. This import did not edit the root lockfile or run
Cargo. Build and behavioral acceptance remain pending the central gate.

## Complete byte inventory

Hashes cover raw file bytes, including line endings. The baseline/final size
and SHA-256 columns make unchanged files independently verifiable.

| Package path | Baseline/final bytes | Baseline SHA-256 | Final SHA-256 |
| --- | ---: | --- | --- |
| `.cargo/config.toml` | 569 / 569 | `4f78bc40c58fd554bffd8c329eb6e03fd827d1ff0f7795562f4390528382078f` | `4f78bc40c58fd554bffd8c329eb6e03fd827d1ff0f7795562f4390528382078f` |
| `.cargo_vcs_info.json` | 94 / 94 | `83096acc1f5c89e893d64e082879427ef231810dfeb7e41252f27264627355ce` | `83096acc1f5c89e893d64e082879427ef231810dfeb7e41252f27264627355ce` |
| `.github/workflows/bindings.yaml` | 1960 / 1960 | `5a7a8d3fffac07bf702039c511f004a0266aec5bb7794c3186575f0f2eb0d5f3` | `5a7a8d3fffac07bf702039c511f004a0266aec5bb7794c3186575f0f2eb0d5f3` |
| `.github/workflows/release.yaml` | 8443 / 8443 | `57fc38802a6272180d83a3f0273e6b6bff711275c0ab851917c42ef3443aeaed` | `57fc38802a6272180d83a3f0273e6b6bff711275c0ab851917c42ef3443aeaed` |
| `.github/workflows/test.yaml` | 3066 / 3066 | `0ed69c316a7aee1bd9a44eeff62f86481890900b23fa29fedb97b6fb287118df` | `0ed69c316a7aee1bd9a44eeff62f86481890900b23fa29fedb97b6fb287118df` |
| `.gitignore` | 166 / 166 | `70313b41ff8652494e6c5264feec423396a2b0cb0deea70ac3751fe5d58d411c` | `70313b41ff8652494e6c5264feec423396a2b0cb0deea70ac3751fe5d58d411c` |
| `BENCHMARKS.md` | 15678 / 15678 | `847bd9f8086b07c1806cea7e34bdea216ab76ee50074aaf1180131a2eeece53a` | `847bd9f8086b07c1806cea7e34bdea216ab76ee50074aaf1180131a2eeece53a` |
| `CHANGELOG.md` | 47935 / 47935 | `7d99da646ab7208adbb2d0a39e89209ea2901c96eb2546d91fdfbd4359ff2212` | `7d99da646ab7208adbb2d0a39e89209ea2901c96eb2546d91fdfbd4359ff2212` |
| `Cargo.lock` | 28799 / 28799 | `6fea1fdf85c4c4e1bde4fcb1bd694ced4d3feb5d18757c104731576f77420aae` | `6fea1fdf85c4c4e1bde4fcb1bd694ced4d3feb5d18757c104731576f77420aae` |
| `Cargo.toml` | 1949 / 1949 | `5d7689582704f41989d0cdf33da76f31d10cdf816982261991fea003aec82fb4` | `5d7689582704f41989d0cdf33da76f31d10cdf816982261991fea003aec82fb4` |
| `Cargo.toml.orig` | 1447 / 1447 | `47b097fce3454a5742a94b814e707f18f41d81bd1ce25462b554268871db9e4b` | `47b097fce3454a5742a94b814e707f18f41d81bd1ce25462b554268871db9e4b` |
| `LICENSE` | 1066 / 1066 | `15b4b2af4b8862541fa14f087d803dba25f5178bf5638551867c4c78bfc66361` | `15b4b2af4b8862541fa14f087d803dba25f5178bf5638551867c4c78bfc66361` |
| `README.md` | 16094 / 16094 | `c5a725ae586bf55ec1ed58559dde9721aa3f852fc2b603f492aaf1a4e903993b` | `c5a725ae586bf55ec1ed58559dde9721aa3f852fc2b603f492aaf1a4e903993b` |
| `benches/data/download.sh` | 716 / 716 | `3bd8bedbe357a77b078b00020f30ac1d07ec7c91efb19a816de6785c99fa004c` | `3bd8bedbe357a77b078b00020f30ac1d07ec7c91efb19a816de6785c99fa004c` |
| `benches/lib.rs` | 2879 / 2879 | `43c49c6e541ee88dd1afd9609c71d1a5e8b92f047081ae56811348ed8dd7dcd6` | `43c49c6e541ee88dd1afd9609c71d1a5e8b92f047081ae56811348ed8dd7dcd6` |
| `benches/match_list/generate.rs` | 6570 / 6570 | `ae2d4e2597984a37a0e4cdbe0658e69b806e1ca8ef7dec75662db5c3433304dd` | `ae2d4e2597984a37a0e4cdbe0658e69b806e1ca8ef7dec75662db5c3433304dd` |
| `benches/match_list/mod.rs` | 8968 / 8968 | `36d2ea4ea5bb655d571407e56909160f973a56ee194587cba92c76cb4fd946c1` | `36d2ea4ea5bb655d571407e56909160f973a56ee194587cba92c76cb4fd946c1` |
| `benches/sort.rs` | 1816 / 1816 | `b353df685cb45e11043e2a61c88fcaac6dc84338ccee72c53bef0e167ae90b35` | `b353df685cb45e11043e2a61c88fcaac6dc84338ccee72c53bef0e167ae90b35` |
| `cliff.toml` | 2243 / 2243 | `f1b4bf8672fcd943b62d73a87e09d5fee27573f3c0c43e5e7590668df0df889d` | `f1b4bf8672fcd943b62d73a87e09d5fee27573f3c0c43e5e7590668df0df889d` |
| `flake.lock` | 1617 / 1617 | `658910cea53eb31318d2d8f2dc6145c93f6912930518dd9bab5b449301a17778` | `658910cea53eb31318d2d8f2dc6145c93f6912930518dd9bab5b449301a17778` |
| `flake.nix` | 1551 / 1551 | `b17083ea6c28521933a67fb26bfa96c8dc226629dc06df386d884b9081923f08` | `b17083ea6c28521933a67fb26bfa96c8dc226629dc06df386d884b9081923f08` |
| `justfile` | 3710 / 3710 | `5408be0a003e0f813df0fb1bb3763accfbcb3a70ff66a107466e81d26b77d949` | `5408be0a003e0f813df0fb1bb3763accfbcb3a70ff66a107466e81d26b77d949` |
| `rustfmt.toml` | 21 / 21 | `aeffdf96c9207bb6b1cb14c082f2f7c702653664c2f23d705b6a6b3de055c9d4` | `aeffdf96c9207bb6b1cb14c082f2f7c702653664c2f23d705b6a6b3de055c9d4` |
| `src/const.rs` | 1001 / 1001 | `92f4b4a43180575e29853b938bd3a5574888ad4e7b768a5f157309964621b87a` | `92f4b4a43180575e29853b938bd3a5574888ad4e7b768a5f157309964621b87a` |
| `src/cpuid.rs` | 6030 / 6030 | `2c300e045dd8c2a8f060f127a3031fe0b99388d0df0554a0680ea24063b157eb` | `2c300e045dd8c2a8f060f127a3031fe0b99388d0df0554a0680ea24063b157eb` |
| `src/k_merge.rs` | 7437 / 7437 | `dae9de14e2d8b1893990ccd5ae38f2678e99ece43238cd13f9e02b1785f2523f` | `dae9de14e2d8b1893990ccd5ae38f2678e99ece43238cd13f9e02b1785f2523f` |
| `src/lib.rs` | 19212 / 19212 | `7ff6fcedff1b04b5bac14b7822c66ec5b2e991dbc50eef22253d0bfee39069a1` | `7ff6fcedff1b04b5bac14b7822c66ec5b2e991dbc50eef22253d0bfee39069a1` |
| `src/literal/algo.rs` | 12905 / 12905 | `5dfd987cd4c78fd8eaf6cd188f7b166aa481065b15feb63a945bb0bc338d467b` | `5dfd987cd4c78fd8eaf6cd188f7b166aa481065b15feb63a945bb0bc338d467b` |
| `src/literal/backend.rs` | 9932 / 9932 | `5cf3ebbc11563039f48fdecb5b1cd7e8a4fe8a3a79c1a29ed1e6c2072b4f435d` | `5cf3ebbc11563039f48fdecb5b1cd7e8a4fe8a3a79c1a29ed1e6c2072b4f435d` |
| `src/literal/mod.rs` | 13619 / 13619 | `79900d9e5376fa189cff931b0385d0923902582ef8dc0edc97f71f3f0e7e0fe4` | `79900d9e5376fa189cff931b0385d0923902582ef8dc0edc97f71f3f0e7e0fe4` |
| `src/literal/rank.rs` | 7035 / 7035 | `c2379d09ab089cdb1a89f422d801ef5374aedb24c622b565ea5fe3454ed1e37e` | `c2379d09ab089cdb1a89f422d801ef5374aedb24c622b565ea5fe3454ed1e37e` |
| `src/matcher/algo.rs` | 16465 / 16465 | `5cc02ec70fdb1023cccec99d6e1922be688adda9281939b775dc4a7a52a6b192` | `5cc02ec70fdb1023cccec99d6e1922be688adda9281939b775dc4a7a52a6b192` |
| `src/matcher/backend.rs` | 8530 / 8530 | `16214e533006dfcf52c72ac1c78f15d1e1ac5fd52981a60be3668beff30a739c` | `16214e533006dfcf52c72ac1c78f15d1e1ac5fd52981a60be3668beff30a739c` |
| `src/matcher/iter.rs` | 7294 / 7294 | `20912e4edd87c735c0230bd4912c7efe218b953cc86fa631a38724f86a281713` | `20912e4edd87c735c0230bd4912c7efe218b953cc86fa631a38724f86a281713` |
| `src/matcher/mod.rs` | 37659 / 37659 | `83b74451e82c6abd853237dc10eafdffb73ff13e9b1802767b60de37a33fb0cf` | `83b74451e82c6abd853237dc10eafdffb73ff13e9b1802767b60de37a33fb0cf` |
| `src/matcher/multi.rs` | 16264 / 16264 | `4c3be59fcbb4b3e2a1a4ea13540bd831ec7f434ad5e6cab3db7ed6716e1c3fe4` | `4c3be59fcbb4b3e2a1a4ea13540bd831ec7f434ad5e6cab3db7ed6716e1c3fe4` |
| `src/matcher/parallel.rs` | 6652 / 6652 | `c9537fb0d22e1dd49ddcf9a280d7842906c3beec7ba9dcdccdff6aca20d1f2ed` | `c9537fb0d22e1dd49ddcf9a280d7842906c3beec7ba9dcdccdff6aca20d1f2ed` |
| `src/pattern.rs` | 14527 / 14527 | `906978b18f214bcc0516b201b02bdb864886884e6a1e7cbca6acad64b5e2ef3c` | `906978b18f214bcc0516b201b02bdb864886884e6a1e7cbca6acad64b5e2ef3c` |
| `src/prefilter/algo/ascii.rs` | 2531 / 2531 | `12a7b5918c0b80e2f6e3b4741ba6a693948ce93a1439d2392693b464e6b257fd` | `12a7b5918c0b80e2f6e3b4741ba6a693948ce93a1439d2392693b464e6b257fd` |
| `src/prefilter/algo/ascii_typos.rs` | 16796 / 16796 | `3a102919d67a0c66c7dcb3101acfa4354ba169804071f72ffa003ca8602bbbcc` | `3a102919d67a0c66c7dcb3101acfa4354ba169804071f72ffa003ca8602bbbcc` |
| `src/prefilter/algo/load.rs` | 1613 / 1613 | `1f7d7944096b04f3d76c80d2e90c0a30500662788d5f099c152b9eecf398d1f6` | `1f7d7944096b04f3d76c80d2e90c0a30500662788d5f099c152b9eecf398d1f6` |
| `src/prefilter/algo/mod.rs` | 1504 / 1504 | `47711cf40ad2abdb2f73592aa19276d49790fcbca736c6ed645dd5d8554536d6` | `47711cf40ad2abdb2f73592aa19276d49790fcbca736c6ed645dd5d8554536d6` |
| `src/prefilter/algo/unicode.rs` | 9787 / 9787 | `7ef4003821f62ae73cce0eae4a4d0ecf9e906e44f3245fe907c22d27fcb97c7b` | `7ef4003821f62ae73cce0eae4a4d0ecf9e906e44f3245fe907c22d27fcb97c7b` |
| `src/prefilter/algo/unicode_typos.rs` | 20168 / 20168 | `2ed3ca4337865a83e714b565b4cc1f645e427e1538f93c614175460e73891bc3` | `2ed3ca4337865a83e714b565b4cc1f645e427e1538f93c614175460e73891bc3` |
| `src/prefilter/backend/avx.rs` | 8548 / 8548 | `8d827ebf0af16995436aa677d7d9b3656389f02500f66a82889c6ac0b8acb1cc` | `8d827ebf0af16995436aa677d7d9b3656389f02500f66a82889c6ac0b8acb1cc` |
| `src/prefilter/backend/avx512.rs` | 1633 / 1633 | `309ea5b73376e9a271897564e7bd2a3bbbf935c2c8e91dca3c45f09ae0b26433` | `309ea5b73376e9a271897564e7bd2a3bbbf935c2c8e91dca3c45f09ae0b26433` |
| `src/prefilter/backend/mod.rs` | 5501 / 5501 | `5093c8e2cc3a24e50c249414e8ff898b127c3b05c0db9bee399a0d875092a49b` | `5093c8e2cc3a24e50c249414e8ff898b127c3b05c0db9bee399a0d875092a49b` |
| `src/prefilter/backend/neon.rs` | 1379 / 1379 | `a8ca58bfe3aed5b31ad9272abb3499863dad1e3b0a7b04ca227ff1b9a373a2ae` | `a8ca58bfe3aed5b31ad9272abb3499863dad1e3b0a7b04ca227ff1b9a373a2ae` |
| `src/prefilter/backend/scalar.rs` | 1506 / 1506 | `9a0e10160aea267ddd20ac947b4aacb888e5b6c460fef33a60602c8a3b0da91f` | `9a0e10160aea267ddd20ac947b4aacb888e5b6c460fef33a60602c8a3b0da91f` |
| `src/prefilter/backend/sse.rs` | 1061 / 1061 | `1b0ea147ab0d9fd1255f54e766ca719f81a253e9b85a14fc3f8e0fbc0468b4c9` | `1b0ea147ab0d9fd1255f54e766ca719f81a253e9b85a14fc3f8e0fbc0468b4c9` |
| `src/prefilter/backend/wasm.rs` | 950 / 950 | `75f1e746d142a4c33a23a02c05e321c54371ebc1c249ffd7504d0d5d1abd9435` | `75f1e746d142a4c33a23a02c05e321c54371ebc1c249ffd7504d0d5d1abd9435` |
| `src/prefilter/mod.rs` | 39930 / 39930 | `d8c5c8dad5c9d5a3110b00162f515030504a8ef8cb9289325588c384a02609dd` | `d8c5c8dad5c9d5a3110b00162f515030504a8ef8cb9289325588c384a02609dd` |
| `src/smith_waterman/algo/ascii.rs` | 7559 / 7950 | `e8005d38ce5ad71b58d23c52d6dead6b6b0c3f986c0aa7c45e703b5ab8c934ce` | `788987c77beaf649b32d0e94a481808522814da3875915722d78caec18b821ad` |
| `src/smith_waterman/algo/ascii_gap.rs` | 3348 / 3348 | `3acc36d8bbc0e2e39f0ecfce3b11c1f23cc72090546d5f83020fe309593c050e` | `3acc36d8bbc0e2e39f0ecfce3b11c1f23cc72090546d5f83020fe309593c050e` |
| `src/smith_waterman/algo/mod.rs` | 7400 / 7860 | `40b14e6d48c422b7221f9013a33f6bd958df27d6c38ab6d1c29f6432500446ea` | `4bfb390d1b087f7946c0637fc5f5e869a3518b80d5f754288067ba364d858cf8` |
| `src/smith_waterman/algo/unicode.rs` | 12378 / 11943 | `53ba2cb8e7fe28a91419d85442e8e2ff11cd135be1f6b2c23ddb6482c11e96b3` | `05d9843d26abaf19960e13f99154f5b5f8ccea61d9fa7a0b777cd4954595478a` |
| `src/smith_waterman/algo/unicode_gap.rs` | 9901 / 9901 | `4996086204b111d1ba656db0db3d7698cb111ab15e298e4867e57513f6794c2e` | `4996086204b111d1ba656db0db3d7698cb111ab15e298e4867e57513f6794c2e` |
| `src/smith_waterman/alignment.rs` | 991 / 991 | `85bb73edaeeb6d4a1872e33501aa97f9803f8f38e9f4e650fddd2cfbd481dd31` | `85bb73edaeeb6d4a1872e33501aa97f9803f8f38e9f4e650fddd2cfbd481dd31` |
| `src/smith_waterman/alignment_iter.rs` | 5955 / 5955 | `f94ceae1b490f0a254b3b94dc7f73093e3e3ee8132b379df4073f033f257f12a` | `f94ceae1b490f0a254b3b94dc7f73093e3e3ee8132b379df4073f033f257f12a` |
| `src/smith_waterman/backend/avx.rs` | 21681 / 21681 | `296bd21820f7e4eed8a329d531e7a01266fc7d696dae1ac3b8f172e7c8b803bc` | `296bd21820f7e4eed8a329d531e7a01266fc7d696dae1ac3b8f172e7c8b803bc` |
| `src/smith_waterman/backend/avx512.rs` | 20384 / 20384 | `2111445d160c234184bd47c6f8f94e88ac506f894b72c98f22662dc2ee86bfa4` | `2111445d160c234184bd47c6f8f94e88ac506f894b72c98f22662dc2ee86bfa4` |
| `src/smith_waterman/backend/mod.rs` | 10571 / 10571 | `38d52214971b0df2cde2f5e76ecac5c6430762a08ede36ba91ea04a7931dffba` | `38d52214971b0df2cde2f5e76ecac5c6430762a08ede36ba91ea04a7931dffba` |
| `src/smith_waterman/backend/neon.rs` | 20909 / 20909 | `272a5d8ecc8cebd2d59ab637164508d699aecfb3addaac59bfdb05cfe212733d` | `272a5d8ecc8cebd2d59ab637164508d699aecfb3addaac59bfdb05cfe212733d` |
| `src/smith_waterman/backend/scalar.rs` | 13707 / 13707 | `7aba77ce606519b29df0d30a82dd304daa7e5c12ec40fa3a1d6254a1b875ae25` | `7aba77ce606519b29df0d30a82dd304daa7e5c12ec40fa3a1d6254a1b875ae25` |
| `src/smith_waterman/backend/sse.rs` | 19594 / 19594 | `0d5735ce5c207bbdd015aa8d6799ccd27be14837325b3a2452be7e9c8a29f3ed` | `0d5735ce5c207bbdd015aa8d6799ccd27be14837325b3a2452be7e9c8a29f3ed` |
| `src/smith_waterman/backend/tests/contract.rs` | 9663 / 9663 | `850d7605ee52a2db9e1fefa7fb4b2d8722a0cecfbcf27a2dfbc8a899d0536145` | `850d7605ee52a2db9e1fefa7fb4b2d8722a0cecfbcf27a2dfbc8a899d0536145` |
| `src/smith_waterman/backend/tests/generator.rs` | 5499 / 5499 | `a1bd5f030dae2c1764330ea4c5594d34580907557a0bb8ec22398e5d7acc072c` | `a1bd5f030dae2c1764330ea4c5594d34580907557a0bb8ec22398e5d7acc072c` |
| `src/smith_waterman/backend/tests/mod.rs` | 400 / 400 | `ddf057ffac688ef8838b4c232340d3fcbff8bc663fa08af0d8ad51a1edd82e5a` | `ddf057ffac688ef8838b4c232340d3fcbff8bc663fa08af0d8ad51a1edd82e5a` |
| `src/smith_waterman/backend/tests/parity.rs` | 11994 / 11994 | `1140bfa3a2c87abe5ef0bac5aff5ffbc8184234a538f50df1a7e0431e6e625fc` | `1140bfa3a2c87abe5ef0bac5aff5ffbc8184234a538f50df1a7e0431e6e625fc` |
| `src/smith_waterman/backend/wasm.rs` | 21568 / 21568 | `1534f35e901a6955989b944e8e61be8dd80996134ae23dbacb1f69d450f48113` | `1534f35e901a6955989b944e8e61be8dd80996134ae23dbacb1f69d450f48113` |
| `src/smith_waterman/greedy.rs` | 6978 / 6978 | `1ec20f6cee6e2d4c826058f6670bc0556b382dec800acca8850e0b397b079ae8` | `1ec20f6cee6e2d4c826058f6670bc0556b382dec800acca8850e0b397b079ae8` |
| `src/smith_waterman/matrix.rs` | 1910 / 1910 | `b205de1ec26ccf6f33754370c3452ba97e9fb5b82a1ff976c990982de7008de2` | `b205de1ec26ccf6f33754370c3452ba97e9fb5b82a1ff976c990982de7008de2` |
| `src/smith_waterman/mod.rs` | 21389 / 21389 | `70cc09c9de0cf98f11af9b70528e2a7ae360e4e09cd7b3b4504dcbd9920e1bc2` | `70cc09c9de0cf98f11af9b70528e2a7ae360e4e09cd7b3b4504dcbd9920e1bc2` |
| `src/sort.rs` | 1962 / 1962 | `35c4176705b4a7d59042ca24215b508f72a7f140b2c94e9bc84853eebc3619c6` | `35c4176705b4a7d59042ca24215b508f72a7f140b2c94e9bc84853eebc3619c6` |
| `tables.toml` | 4196 / 4196 | `b9f229915e739b4b833e5b3d94c6d08d441466e6845fa31ce2d3df54a78e2bc5` | `b9f229915e739b4b833e5b3d94c6d08d441466e6845fa31ce2d3df54a78e2bc5` |
| `tests/api_properties.rs` | 26253 / 26253 | `faafed7a5c971cd45c0a9b16d5a8b53e77e528375cc7198e3831c8f476a99a24` | `faafed7a5c971cd45c0a9b16d5a8b53e77e528375cc7198e3831c8f476a99a24` |
