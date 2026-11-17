# Arsitektur Notefart

Status: editor hybrid panel + tab, registry dokumen/history bersama dan split dua orientasi sudah berjalan. Redesign bergaya superfile menyediakan frame penuh, ikon file dan chrome responsif; field editable serta bantuan F1 tetap tersedia. Validasi penggunaan harian dan matriks dukungan lebih luas masih terbuka.

Nama produk/package Cargo tetap Notefart/`notefart`; satu-satunya target executable adalah `naf`, dideklarasikan melalui `[[bin]]` dengan entry point `src/main.rs`. Tanpa path memulihkan sesi default; `--no-session` atau argumen file/folder memakai editing terpisah tanpa I/O snapshot. Nama binary tidak menentukan direktori konfigurasi: namespace preferensi tetap `notefart`. Tidak ada alias executable lama.

## Prinsip

- Pisahkan mesin editing, state aplikasi, I/O, dan rendering.
- Satu dokumen memiliki satu sumber teks; panel hanya menyediakan tampilan terhadap dokumen itu.
- Gunakan desain sederhana dahulu, bukan plugin framework atau event bus generik.
- Render area terlihat; hindari menyalin seluruh dokumen pada setiap input atau frame.
- Optimasi didasarkan pada pengukuran, tanpa mengorbankan integritas dokumen.

## Batas komponen

| Komponen produksi | Tanggung jawab |
| --- | --- |
| `app` | Lifecycle, dispatch, fokus, modal bertarget ID stabil, close/quit continuation dan routing clipboard |
| `document` | Rope, transaksi edit, history/saved checkpoint, generation teks dan baris invalidasi |
| `workspace` | Registry DocumentBuffer, panel/tab/view, transaksi remap, dedup file dan state explorer |
| `layout` | Topologi split, minimum subtree, ratio integer, divider dan collapse/restore geometry |
| `chrome` | Geometry bersama header/dock, outer/inner frame, sidebar, gutter dan minimum panel; tanpa state/I/O |
| `explorer` | Tree lazy, selection/scroll, refresh atomik, listing berbatas dan symlink policy |
| `search` | Query KMP literal forward/reverse, boundary grapheme, Replace All satu transaksi |
| `syntax` / `build.rs` | SyntaxSet immutable bersama; grammar TS/TSX di-link saat build dan dump-nya di-embed, tanpa YAML/network saat runtime |
| `highlight` | Deteksi syntax, cache viewport, parse/scope checkpoints, invalidasi generation |
| `theme` | Palet semantik precomputed, degradasi warna dan pemetaan ikon |
| `config` | Preferensi TOML pengguna berbatas, resolusi lokasi XDG/HOME dan penyimpanan atomik eksplisit sebelum TUI |
| `session` | Lock sesi default, metadata snapshot berbatas, payload Rope streaming, checksum dan penyimpanan atomik |
| `clipboard` | Pilihan internal/Wayland/X11, copy/paste eksplisit, transport helper berbatas tanpa shell atau I/O render |
| `input` | Decoder keyboard/mouse/bracketed paste berbatas |
| `field` | String field, caret/anchor byte UTF-8, boundary grapheme, revision dan replacement berbatas |
| `field_ui` | Window field mengikuti caret, seleksi visual, escape dan pemetaan sel ke byte sumber |
| `help` | Overlay bantuan, wrapping streaming, scroll/page dari layout yang sama dan tombol kembali |
| `shortcuts` | Katalog kelompok shortcut bersama untuk CLI dan overlay F1 |
| `ui` | Layout/render Ratatui, geometry hit, status/helper dan dialog tanpa filesystem/parser |
| `file_io` | Format, baseline/fingerprint, metadata dan save atomik |
| `terminal` | Setup/restore raw mode, alternate screen, mouse dan bracketed paste |

Nama tersebut adalah batas tanggung jawab, bukan kewajiban membuat crate terpisah.

## Model state produksi

- ID dokumen/view/panel/divider memakai domain `u64` monotonik yang tidak dipakai ulang.
- `Document`: Rope, generation teks, saved checkpoint, delta history, notifikasi `TextChange` dan cache geometri sparse turunan per dokumen.
- `DocumentBuffer`: registry owner Document, path/label, baseline disk, format dan read-only.
- `Tab`: document ID, cursor, selection anchor, preferred column dan viewport sendiri; tanpa salinan Document.
- `Pane`: tab terurut dan active view ID. Dokumen yang sama paling banyak sekali per panel, tetapi dapat menjadi view di panel lain.
- `SplitTree`: arena biner berorientasi Columns/Rows, divider ID dan ratio fixed-point; layout menghitung minimum subtree tanpa mengubah ratio tersimpan.
- `Workspace`: registry, panel/tree, panel tujuan terakhir serta explorer/fokus.

### Invariant

1. Membuka file yang identitasnya sama memakai dokumen yang sama, termasuk pembukaan dari panel lain. Identitas file dan alias path ditangani di lapisan I/O; string path saja tidak cukup.
2. Cursor, selection, dan scroll bersifat lokal per view; teks dan undo bersifat bersama per dokumen.
3. Setiap edit adalah transaksi dengan delta yang dapat memetakan ulang posisi semua view pada dokumen itu.
4. Dirty state membandingkan state sekarang dengan saved checkpoint, bukan hanya flag yang selalu aktif setelah mengetik. Undo kembali ke checkpoint harus menghapus penanda dirty.
5. Dokumen dirty tidak dibuang saat view terakhir ditutup tanpa keputusan pengguna. Keluar aplikasi memeriksa seluruh dokumen dirty, bukan hanya tab aktif.
6. Hasil pekerjaan tertunda hanya diterapkan jika identitas dan revision sumbernya masih sesuai.

## Aliran event

1. Backend terminal menghasilkan event keyboard, mouse, paste, resize, atau lifecycle.
2. Input memetakan event berdasarkan fokus dan modal aktif menjadi command seperti InsertText, SaveDocument, atau SplitPane.
3. Handler memperbarui state melalui transaksi atau meminta I/O.
4. Delta edit memperbarui posisi view dan menginvalidasi cache yang terpengaruh.
5. UI menggambar snapshot state yang konsisten; rendering tidak melakukan operasi file atau clipboard.

Gunakan satu event loop sebagai pemilik state pada awal implementasi. Pekerjaan berat boleh dipindahkan ke worker setelah diukur; hasilnya masuk kembali sebagai event bertanda dokumen/revision. Tidak perlu runtime async hanya untuk rendering TUI.

## Teks dan koordinat

- Buffer produksi memakai Ropey; offset transaksi adalah char Unicode, bukan byte UTF-8.
- Bedakan offset byte UTF-8, posisi teks, grapheme cluster, dan kolom sel terminal.
- Navigasi/delete karakter harus menghormati grapheme cluster. Highlight berbasis byte tidak boleh memotong batas UTF-8.
- Mapping mouse ke cursor memakai layout baris yang sama dengan rendering, termasuk tab, scroll horizontal, dan karakter lebar.
- Cache geometri menyimpan paling banyak 8 baris panjang dengan checkpoint batas grapheme sekitar setiap 1024 char serta satu frontier terakhir. Offset byte/char relatif baris dan kolom terminal absolut menjaga fase tab; baris pendek tidak mengusir cache baris panjang. `column_of`/`position_at` mencari checkpoint, dan renderer memakai `cells_from_column` dengan clipping tetap.
- Sebelum edit/undo/redo, checkpoint pada baris berubah dipotong mulai grapheme sebelumnya dan cache baris sesudahnya dibuang. Prefix yang tidak berubah boleh dipakai lintas revision; tail selalu dihitung ulang, termasuk jika combining/ZWJ menyatu dengan cluster sebelumnya. Clone Document tidak menyalin cache turunan.
- Resumption dan transisi chunk membentuk konteks `GraphemeCursor` dari slice baris utuh, bukan suffix terpotong. Pre-context dipasok tepat sampai offset yang diminta agar pairing regional indicator, combining dan ZWJ konsisten. Seek dingin/lookbehind Unicode panjang tetap dapat mahal; ini bukan indeks seluruh dokumen atau kuota CPU/RAM.
- Usulan awal memakai scroll horizontal tanpa soft wrap untuk mengurangi kompleksitas koordinat; soft wrap belum diputuskan.
- Bracketed paste masuk sebagai satu transaksi undo, bukan deretan shortcut.
- Navigasi kata dan double-click mengelompokkan grapheme berdasarkan karakter pertamanya: alphanumeric/underscore, whitespace, atau punctuation. Pemindaian memakai iterator char Rope berurutan dan memeriksa boundary grapheme hanya saat kelas berubah; scalar di dalam combining/Prepend/ZWJ tidak menjadi batas kata. Ctrl+Left/Right menghitung arah yang diperlukan saja, termasuk whitespace lintas baris, tanpa menyalin teks atau membuat indeks tambahan.

## I/O dan clipboard

Load/save mengembalikan error yang bisa ditindaklanjuti. Save memakai snapshot revision tertentu; sukses hanya menandai snapshot itu tersimpan, tidak membersihkan edit yang datang sesudahnya. Kebijakan konflik dan penggantian file ada di [SECURITY.md](SECURITY.md).

Editor memiliki satu clipboard yang dibagi editor/field sesuai fokus. Mode Auto memilih helper lokal `wl-copy`/`wl-paste` atau `xclip`; tanpa backend atau pada Auto SSH memakai internal dengan alasan yang terlihat. Mode System tidak menyamarkan kegagalan sebagai sukses internal. Baca hanya saat paste eksplisit, tulis hanya copy/cut; sumber cut dipertahankan jika copy gagal. Read eksternal maksimum 1 MiB UTF-8; gagal/kosong tidak mengganti seleksi dan tidak memakai data internal lama. Deadline I/O membatasi helper yang macet; owner desktop yang berhasil tetap boleh melayani clipboard.

Preferensi `theme`, `color`, `icons`, `clipboard` dimuat sekali sebelum terminal masuk raw mode: bawaan < TOML pengguna < CLI. Tidak ada project config atau eksekusi dari konfigurasi. `--save-config` adalah operasi simpan preferensi lalu keluar; startup biasa tidak menulisnya. Path default XDG_CONFIG_HOME absolut atau HOME absolut, bukan CWD. File invalid gagal jelas; `--no-config` membypassnya. Ini bukan session persistence atau autosave.

## Dependency awal dan batas pemilihannya

| Keperluan | Pilihan batch awal | Alasan / status |
| --- | --- | --- |
| UI | Ratatui 0.29.0 | Editor dan diagnostik; bukan versi terbaru |
| Kontrol terminal / tipe event | Crossterm 0.28.1 | Backend kompatibel; decoder event Crossterm tidak digunakan |
| Signal | signal-hook 0.3.18 | Handler flag SIGTERM/SIGHUP/SIGINT; cleanup di thread utama |
| Buffer produksi | Ropey 1.6.1, default-features=false + simd | Document, transaksi, history, dan navigasi/cell chunk-safe |
| Segmentasi | unicode-segmentation 1.13.3 | Boundary grapheme; versi data Unicode 17.0 |
| Lebar sel | unicode-width =0.2.0 | Pin wajib Ratatui 0.29; data Unicode 15.1, tidak identik dengan segmentasi |
| Highlight produksi | Syntect 5.3.0, regex-onig | Oniguruma native untuk parsing; default grammar + TS/TSX berlisensi di-link saat build |
| Clipboard | String internal + helper wl-clipboard/xclip | Tanpa crate clipboard baru; Wayland/X11 lokal, OSC 52 belum tersedia |
| Input dan syscall aman | Rustix 1.1.6 | Poll/read berbatas pada thread utama dan operasi fd filesystem |
| Atomic file replacement | tempfile 3.27.0 | Tempfile satu direktori dan persist/no-clobber |
| Fingerprint | BLAKE3 1.8.7, pure | Deteksi perubahan isi, bukan mtime saja |
| Atribut filesystem | xattr 1.7.0 | Copy atribut/ACL berbasis fd atau gagal eksplisit |
| Preferensi | toml 1.2.1, std/parse tanpa serde | Parser TOML standar untuk empat key, bukan parser buatan sendiri; MIT OR Apache-2.0, MSRV 1.85 ([metadata](https://crates.io/crates/toml/1.2.1)) |

Cargo.lock mengunci dependency. Rustix/tempfile/xattr berlisensi permisif MIT/Apache, BLAKE3 CC0/Apache; lisensi proyek tidak ditetapkan implisit. Backend kini memakai onig 6.5.3/onig_sys 69.9.3 (wrapper MIT, library C BSD-2-Clause); notice di `assets/licenses/Oniguruma.txt`. Grammar TypeScript/TSX beserta provenance commit dan MIT notice ada di `assets/syntaxes/`. Distribusi binary harus menyertakan notice terkait; audit seluruh dependency/platform tetap pekerjaan terpisah. Tree-sitter tidak ditambahkan.

## Strategi verifikasi

- Unit test untuk transaksi edit, pemetaan posisi, checkpoint save, dan perubahan split tree.
- Pengujian filesystem terisolasi untuk round-trip format, konflik, dan kegagalan save.
- Skenario terminal nyata untuk input, Unicode, mouse, resize, clipboard, dan restorasi terminal.
- Ukur waktu input/render, penggunaan memori, dokumen besar, dan baris panjang sebelum menetapkan target kinerja.
- Pengukuran TUI memisahkan waktu teks pertama terlihat, acknowledgement input, CPU proses, RSS saat observasi dan VmHWM. Timer dimulai sebelum spawn; membaca output tidak menambahkan jendela polling wajib. Ukuran file dan panjang satu baris diperlakukan sebagai dimensi berbeda; hasil, hash fixture dan keterbatasan di [ROADMAP.md](ROADMAP.md#hasil-status-panjang-dan-profil-dokumen-besar).

Tahapan dan kriteria kelulusan berada di [ROADMAP.md](ROADMAP.md).

## Keputusan batch validasi terminal

Batch pertama menghasilkan `notefart --probe`, bukan editor file, dan saat itu tanpa argumen menampilkan bantuan. Batch berikutnya menambahkan editor default; `--help` tetap bantuan. Tidak ada explorer/tab/split palsu sebagai pengganti fitur yang belum tersedia.

- Integrator memiliki Cargo.toml, entry point, integrasi, dan dokumentasi keputusan.
- Worker terminal/UI memiliki `src/probe.rs` dan `src/terminal.rs`; antarmuka ke entry point hanya `probe::run() -> std::io::Result<()>`.
- Worker teks memiliki `examples/text_probe.rs`: pengukuran rope versus String, boundary Unicode, dan round-trip transaksi edit/undo dalam memori.
- Worker highlighting memiliki `examples/highlight_probe.rs`: parsing multiline dan pengukuran Syntect, disertai perbandingan arsitektural dengan Tree-sitter.
- Runtime awal memakai Ratatui 0.29 dan Crossterm 0.28 yang kompatibel. Versi tersebut dipilih sebagai pasangan API yang diketahui, bukan klaim versi terbaru. Toolchain validasi tersedia: Rust/Cargo 1.99.1, target Linux x86_64.
- Evaluasi memakai Ropey 1.6.1, unicode-segmentation 1.13.3, unicode-width =0.2.0 dan Syntect 5.3 regex-fancy; Ropey dan Syntect kini dependency produksi.
- Probe terminal harus mencatat input, mouse, paste, dan resize secara aman, memakai bounded event log, serta memulihkan terminal saat exit, error, dan signal yang ditangani. Mode ini tidak menyimpan input ke disk atau clipboard.

Ropey menjadi buffer produksi setelah validasi; Syntect terhubung pada tahap workspace. Editor diuji lewat PTY lokal, probe dengan input baru juga diuji di tmux terisolasi. Integrasi clipboard lokal ditambahkan pada batch preferensi; SSH, font/glyph fisik dan matriks platform universal belum terverifikasi.

### Hasil evaluasi yang diamati

Satu pengukuran release pada Linux x86_64/Rust 1.99.1; angka berikut bukan jaminan performa lintas mesin atau ukuran dokumen.

| Workload | Ropey | String |
| --- | --- | --- |
| Konstruksi 233.000 byte, 25 iterasi | 5,561 ms total | 0,490 ms total |
| Edit tengah + undo, 1.000 siklus | 2,489 ms total | 34,647 ms total |
| Clone snapshot, 500 iterasi | 0,021 ms total | 9,640 ms total |

String menerima offset byte yang sudah diketahui, jadi tidak dibebani pencarian baris/karakter pada benchmark. Ropey unggul pada workload edit/snapshot ini tetapi lebih mahal pada konstruksi. Fixture memvalidasi 237 chunk dan grapheme ZWJ yang melintasi seam chunk; tidak memakai identitas pointer/internal chunk sebagai syarat correctness.

Syntect: load syntax set 2,136 ms; cold parse 110,178 ms; warm parse+mapping semantik 40,807 ms/pass untuk 5.320 byte/280 baris, 20 iterasi. Loader/cold init diukur terpisah. UTF-8, komentar multiline, perubahan terminator, pemulihan kategori, dan EOF tanpa newline lolos. Tree-sitter dibandingkan secara arsitektural, **tidak dibenchmark**.

Syntect dipilih untuk distribusi grammar lebih sederhana. Parsing/cold init tidak berjalan di rendering. Produksi kini memakai checkpoint setiap 128 baris, invalidasi generation dan kebijakan 16 KiB/baris; angka contoh di atas tetap bukan jaminan latensi editor.

Rujukan primer: [Syntect](https://github.com/trishume/syntect), [lisensi aset Sublime Packages](https://github.com/sublimehq/Packages/blob/st3/LICENSE), [Tree-sitter incremental parsing](https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html), dan [Ropey](https://github.com/cessen/ropey).

### Lifecycle input editor dan probe

`src/input.rs` menggantikan decoder Crossterm dan thread pembaca lama sepenuhnya. Readiness poll + read maksimal 4096 byte memproses decoder streaming pada thread utama. Paste dibatasi 1 MiB; overflow/invalid UTF-8 dibuang utuh dan tidak diubah menjadi command. CSI/UTF-8/prefix delimiter parsial tetap berbatas; timeout tidak membuang prefix paste, dan OSC/string-control dibuang sampai terminator.

Loop memeriksa signal dan ukuran terminal saat idle/pasca-poll. Snapshot geometry layout dipertahankan terpisah dari validitas hit target: perubahan tree/tab/scroll/resize menolak mouse stale sampai frame baru, tanpa mengembalikan scroll explorer ke selection. Gutter yang berubah merekonsiliasi cursor. Dialog umum di bawah 20×6 dan Find/Replace di bawah 40×10 menahan aksi tersembunyi.

### Komponen editor workspace

- `Document` memiliki Rope/history dan saved state ID; `revision()` teks monotonik terpisah dari checkpoint history, termasuk undo/redo. `changed_from()` mencatat baris awal transaksi terakhir. Cache dengan generation yang terlewat menginvalidasi konservatif dari nol.
- `Document::cells` menghasilkan grapheme display aman, offset char, kolom dan lebar. Normal text dipinjam dari chunk; seam/escape dapat mengalokasikan lokal. Render/cursor/hit/word selection memakai boundary yang konsisten.
- `DocumentBuffer` memiliki teks/path/baseline/format/read-only sekali; `Tab` hanya memiliki document ID dan state view. Alias dev/inode/canonical tetap shared lintas panel; missing path memakai ancestor existing tanpa meruntuhkan `..` melalui symlink. Atomic save memperbarui baseline bersama.
- New/Open menambah/mengaktifkan tab di panel tujuan. Close satu shared view tidak meminta discard; final view dirty memakai guard. Panel kosong non-terakhir dihapus dari tree, panel terakhir mendapat blank. Quit memeriksa dokumen sekali dan tidak memalsukan checkpoint saved. Save As menolak target dokumen lain, bukan view lain dokumen sendiri.
- Explorer membaca root/anak secara lazy, folder-first dan stabil, termasuk dotfiles. Refresh membangun tree pengganti sebelum commit. Ekspansi membuka setiap komponen relatif dari descriptor root dengan `openat`/`NOFOLLOW`; metadata entri memakai `statat` tanpa follow. Error mempertahankan tree usable sebelumnya.
- Query menyimpan pattern/failure table KMP forward/reverse sekali. Kedua arah streaming Ropey dan hanya menerima endpoint grapheme. Replace All dua pass: preflight ukuran jendela hasil maksimum 64 MiB, komposisi hanya jendela yang berubah, lalu satu `Document::replace`; tidak ada daftar semua range match.
- Hasil Find membawa pane/view/document ID, generation, query dan range; F6 memvalidasi semuanya terhadap seleksi. Pindah view/panel menginvalidasi hasil stale. Query literal case-sensitive, wrap sekali, field 4096 byte.
- `Highlighter` meminjam SyntaxSet immutable process-wide dari `syntax::load()` (`LazyLock`), dengan ParseState/ScopeStack terpisah per dokumen. `lines()` hanya berisi union viewport terlihat. Baris yang baru keluar viewport dipindah tanpa clone span ke FIFO recent, maksimum 512 baris dan 2 MiB payload per dokumen; gap besar antarpanel tidak ditokenisasi untuk mengisi cache.
- Checkpoint parse/scope tiap 128 baris, budget 200 baris dan slice kooperatif 4 ms per update. Render tidak menjalankan parser; event loop melukis frame pertama sebelum cold highlight dan tidak menjalankan beberapa slice sebelum frame/input yang sama. Idle 16 ms melanjutkan pekerjaan pending, lalu kembali ke poll normal. Baris >16 KiB/error parser memberi warning satu kali dan menghentikan propagasi; satu operasi regex tidak dapat dipotong oleh slice.
- `theme` memilih palet precomputed pada startup dan ketika perubahan tampilan diterapkan, bukan per frame; kontras pasangan seleksi/fokus RGB Dark/Light dan xterm-256 diuji. Canvas memakai `Color::Reset` pada semua palet, sehingga kontras foreground terhadap latar terminal tidak diketahui aplikasi. ANSI16/Terminal/default colors tetap bergantung palet terminal; mono mempertahankan reverse/bold/marker. Tidak ada deteksi/pemasangan font otomatis.
- `file_io` tetap memisahkan LF dalam memori dari BOM/LF/CRLF disk. Save sinkron memakai tempfile satu direktori, baseline hash/identitas, metadata copy + pemeriksaan ulang, fsync, atomic persist dan directory fsync. Persist mengganti nama, bukan descriptor: nama temporary diikat ulang ke dev/inode/nlink descriptor penulis sebelum rename dan direktori tujuan diverifikasi menunjuk inode yang sama sesudahnya; ketidakcocokan menjadi `FileError::Substituted`. Celah check→rename pada direktori yang dapat ditulis proses lain tidak dieliminasi.
- Batas file/paste bukan batas seluruh RSS/history/clipboard. Filesystem sinkron dapat menahan UI. Pada akhir batch tahap 1, recovery, clipboard eksternal dan konfigurasi persisten belum tersedia; clipboard/preferensi kini ditambahkan terpisah, recovery tetap belum tersedia. `examples/` tetap evaluasi terpisah.

## Kontrak switcher tampilan runtime

- F11 atau Alt+P membuka Appearance dari editor/explorer, hanya saat tidak ada modal lain. F12 tetap Close pane. `App` menyimpan `Options` yang diminta pengguna sebagai sumber preferensi runtime; `Theme` tetap palet turunan, dan `Screen.icons` berasal dari `options.icons`. Auto tidak disimpulkan kembali dari warna palet yang sudah terdegradasi.
- Modal Appearance memiliki `draft: Options` dan `focused: usize`. Fokus 0–2 adalah theme/colors/icons; 3 Apply dan 4 Cancel. Pilihan tema Dark → Light → Terminal, warna Auto → Truecolor → Ansi256 → Ansi16 → None dan ikon Unicode → Nerd → Ascii berputar dua arah tanpa alokasi. Clipboard tidak menjadi pilihan menu dan tidak diubah saat Apply.
- Perubahan hanya pada draft sampai Apply; tidak ada preview atau I/O konfigurasi. Apply menerapkan tiga preferensi visual dan membangun `Theme::new(draft.theme, draft.colors)` hanya jika theme/colors berubah; pergantian ikon saja tidak membangun ulang palet. Tidak ada reparse syntax, transaksi dokumen atau perubahan backend clipboard. Esc membuang draft. F1 memakai `help_return` yang sudah ada untuk mempertahankan draft/fokus, tanpa stack modal baru.
- `Dialog::Appearance { draft, focused }` adalah kontrak render; `MIN_APPEARANCE_SIZE` = 40×10 dan `APPEARANCE_FOCUS_COUNT` = 5 berasal dari `ui` dan dipakai controller serta renderer. Di bawah minimum, hanya Esc/F1 dilayani; tidak ada target Apply/cycle tersembunyi. `CommandHit` tetap KeyEvent statis, bukan parsing label: t/T, c/C, i/I memilih row lalu next/previous; a/A menerapkan langsung; Esc membatalkan dan F1 membuka bantuan.
- Tab/Shift+Tab atau Up/Down memindahkan fokus; Left/Right mengganti nilai row atau berpindah antara tombol Apply/Cancel. Space mengganti nilai row atau mengaktifkan tombol; Enter menerapkan kecuali fokus Cancel. Mouse prev/next memakai mnemonic row yang sama; tombol Apply memakai a, bukan Enter yang bergantung fokus.
- Open, Apply dan Cancel membatalkan capture serta hit lama sampai redraw. Menutup Appearance lewat keyboard tidak memaksa viewport kembali ke caret; cursor, seleksi, scroll, dirty/history, struktur panel/tab dan fokus editor/explorer tetap. Resize masih boleh mengikuti kebijakan layout yang sudah ada. Input edit/save/clipboard di belakang menu tidak diteruskan.
- Berlaku hanya untuk proses berjalan. Snapshot sesi tidak menyimpan pilihan tampilan; konfigurasi persisten tetap memakai mekanisme eksplisit `--save-config` sebelum TUI. Font tetap di terminal, Nerd opt-in, dan canvas tetap `Color::Reset`.


## Keputusan shared-view tahap 3

- Satu registry memiliki Document/history/path/baseline per identitas file; tab hanya memiliki document ID dan state view, panel memiliki daftar tab. Tidak memakai clone Document untuk split.
- Split tree rekursif memakai orientasi Columns (kiri/kanan) dan Rows (atas/bawah), ratio integer tersimpan dan minimum per subtree. Resize terminal boleh menyembunyikan explorer/panel nonaktif, tidak menghapus tree atau menulis ulang ratio.
- Transaksi edit/undo/redo mengirim splice char sebelum/sesudah ke semua view. Cursor/anchor memakai affinity kanan; anchor viewport kiri. Posisi dalam range yang diganti dipetakan ke tepi penggantian lalu dinormalisasi ke boundary grapheme; Replace All tetap satu splice jendela komposisi.
- Highlight cache per document menerima union viewport yang terlihat, bukan range besar yang mencakup gap antarpanel. Rendering tetap tanpa parser/I/O.
- Close view/panel memeriksa dokumen yang kehilangan view terakhir; quit memeriksa dokumen sekali. SaveAs/Reload memperbarui registry bersama, modal memakai ID stabil. Build dan 178 unit test serta alur PTY lintas panel dibuktikan; detail di ROADMAP.md.

## Keputusan pemolesan field dan bantuan

- Posisi `Document`/view tetap offset scalar; `Field` memakai byte UTF-8 pada boundary grapheme. `FieldView` meminjam teks, caret dan range seleksi tanpa menyalin isi field. Mutasi yang menggabungkan grapheme menempatkan caret sesudah cluster hasil, bukan sebelum teks yang baru dimasukkan.
- App memvalidasi path sebelum mutasi dan menormalisasi CRLF/CR untuk paste dokumen/pencarian. Batas field 4096 byte dihitung sesudah normalisasi dan pengurangan seleksi; penolakan tidak mengubah isi maupun seleksi. Revision hanya berubah saat teks berubah, sehingga navigasi caret tidak membatalkan hasil Find aktif.
- Clipboard dibagi antara editor dan field, tetapi copy/cut/paste mengikuti fokus modal dan backend terpilih. Undo/redo dokumen tidak diteruskan melalui modal. Field tidak memiliki history undo tersendiri.
- `field_ui` merender grapheme/escape dan menghasilkan `FieldHit` dengan koordinat absolut serta offset byte asli, termasuk setiap sel grapheme lebar. Geometry memakai pool hit agar buffer stop map dapat dipakai ulang; perubahan field/fokus/modal menginvalidasi hit sampai frame baru.
- Field path/Find/Replace menampilkan indikator overflow pasif `‹`/`›` (`<`/`>` pada ASCII) ketika window dasar menyembunyikan teks. Dua gutter satu sel dicadangkan hanya jika tersedia minimal tiga sel nilai; sisi tanpa teks tersembunyi digambar kosong. Window/caret dan `FieldHit` memakai area teks setelah gutter, sehingga indikator/prefix tidak memilih byte atau mengganti fokus. Pada satu/dua sel nilai, indikator dihilangkan demi caret; clipping grapheme tetap memakai marker lama dan offset byte asli. Indikator bukan scrollbar interaktif, tidak menambah autoscroll/history atau mengubah sanitasi.
- `Modal::Help` menangguhkan satu modal dalam `help_return`, tanpa rekursi atau clone dokumen. Esc/F1 mengembalikannya dan mempertahankan `after_save`; command editor tidak dijalankan di belakang bantuan.
- `help` menelusuri katalog statis secara streaming tanpa daftar seluruh baris di heap. Wrapping/layout yang sama menentukan draw, scroll limit dan ukuran page; offset lama di-clamp ketika ukuran berubah. `shortcuts::SECTIONS` juga dipakai oleh `--help`.
- `Screen` membawa path, format dan dirty sebagai metadata terpisah dari pesan operasi. Buffer status dipakai ulang, metadata menyusut pada ruang sempit dan error diprioritaskan. Aksi konfirmasi memakai `ChoiceKind`/KeyEvent statis, bukan parsing nama file atau status menjadi command.

## Kontrak integrasi P1

Kontrak implementasi batch P1; bukti fungsional dan batas validasi ada di [ROADMAP](ROADMAP.md#hasil-p1-aksesibilitas-dan-pencarian):

- Aplikasi memiliki fokus tombol per modal konfirmasi, sehingga prompt baru selalu Cancel dan suspend/return F1 mempertahankan fokus. Renderer menerima indeks fokus serta target path pinjaman; guard input dan renderer memakai perhitungan ukuran yang sama.
- `Query::match_index(document, limit)` menghasilkan awal match scalar terurut dan flag `has_more`, termasuk match overlapping yang valid pada kedua batas grapheme. Penyimpanan hanya prefix berbatas; tidak menyalin dokumen atau menyimpan semua range.
- Cache pencarian aplikasi mengikat compiled query dan indeks pada document ID/revisi/query; reload menginvalidasi cache secara eksplisit karena mengganti `Document` dapat memakai ulang nomor revisi. Feedback dibuat saat aksi Find, dipinjam renderer, dan tidak ditampilkan jika konteks dokumen/view/seleksi sudah stale. Find menavigasi start match overlapping pada kedua arah; replacement/undo tetap memakai kontraknya sendiri dan navigasi tidak dibatasi prefix.
- `Theme` menjadi satu-satunya pemilik resolusi warna konfigurasi/CLI/environment. Sesudah bootstrap dan sebelum terminal editor masuk, backend Crossterm diizinkan mengeluarkan warna tema agar tidak menerapkan `NO_COLOR` kedua kali. Auto tetap menghormati `NO_COLOR`; None memakai token `Reset` dan reverse/marker, sedangkan override eksplisit tidak dibatalkan backend. Regresi output backend mengisolasi tiap kasus dalam subprocess, bukan memutasi environment/flag global suite paralel.
- Kepemilikan batch: renderer/helper/tab/katalog shortcut di `ui`/`shortcuts`, enumerasi match di `search`, state/action serta seluruh caller lintas modul di `app`, kebijakan output backend di `terminal`. Bukti terminal/font, client tmux, SSH loopback dan penggunaan panjang menutup tahap 4 pada matriks Linux teruji yang disetujui pengguna, bukan platform atau client fisik universal.

## Kontrak indent/dedent blok

Kontrak indentasi telah terimplementasi dan lulus tes serta alur TUI terarah; [bukti dan batas](ROADMAP.md#hasil-indent-dan-dedent-blok):

- Tab tanpa seleksi multibaris tetap menyisipkan spasi sampai tab stop empat kolom, termasuk mengganti seleksi satu baris seperti sebelumnya. Tab dengan seleksi yang melintasi newline menambah empat spasi pada awal setiap baris yang tersentuh, termasuk baris kosong.
- Shift+Tab (`BackTab`, atau Tab dengan modifier Shift) mengurangi indentasi baris aktif atau baris terpilih: satu tab awal, atau hingga empat spasi ASCII awal. Kombinasi spasi lalu tab hanya menghapus prefix spasi; karakter non-indentasi tidak dihapus. Endpoint penghapusan tetap boundary grapheme.
- Range seleksi bersifat setengah-terbuka: baris berikutnya tidak ikut jika endpoint tepat pada awalnya. Arah seleksi dan posisi logis cursor/anchor dipetakan melalui tiap perubahan prefix; operasi tidak mengganti isi seleksi dengan spasi atau meruntuhkan seluruh posisi view ke ujung blok.
- Seluruh prefix dalam satu perintah merupakan satu transaksi/history checkpoint. Undo/redo tetap mengikuti aturan cursor command yang sudah ada, tetapi memulihkan seluruh batch, termasuk tab versus spasi asli; shared view dan anchor viewport menerima pemetaan tiap prefix. No-op dedent tidak membuat dirty/revisi/history baru atau membuang redo.
- `Document::indent_lines(range, dedent)` menerima offset scalar/grapheme seperti `replace`, memvalidasi sebelum mutasi, menyimpan metadata prefix ringkas dan tidak menyalin isi blok/dokumen. `last_change()` menyediakan pemetaan transaksi tunggal atau batch yang meminjam metadata history; jalur replace tunggal tidak menambah alokasi batch.
- Workspace memakai kebijakan view eksplisit: mengikuti cursor edit, memetakan cursor sambil menghapus seleksi, atau mempertahankan seleksi. Semua caller bool lama dimigrasikan tanpa shim; aplikasi memakai kebijakan mempertahankan seleksi untuk indent/dedent.
- Read-only tetap menolak edit tanpa mutasi. Routing modal, field dan explorer diproses sebelum shortcut editor; Tab/Shift+Tab pada dialog tetap navigasi fokus, bukan indentasi dokumen. Mutasi menginvalidasi hasil Find/hit mouse dan memicu highlighting melalui jalur edit yang sama.


## Geometry frame dan chrome responsif

- `chrome` dipakai langsung oleh `ui`, `app` dan `workspace`; minimum panel tidak diduplikasi. Outer panel mencakup frame; inner membagi tab, gutter, teks dan jalur scrollbar editor. `Geometry.sidebar_panel` adalah kartu luar, `sidebar_parent` hit parent yang aktif, dan `sidebar` hanya baris tree di bawahnya. Parent tetap dialokasikan di `/`, tetapi hit-nya tidak diterbitkan.
- Minimum panel max(12, digit gutter + 5) kolom ×4 baris. Chrome penuh dipilih bila lebar ≥40, tinggi ≥18 dan body setinggi terminal dikurangi **7 baris** masih memuat tree. Header 3 baris, status 3 baris, helper tanpa frame 1 baris; tidak ada baris kosong antara header/body/dock. Compact tetap header satu baris dan dock satu–dua baris.
- `chrome::pane_areas` menyisakan satu baris kosong antara tab dan gutter/teks bila tinggi panel minimal 5; pada minimum 4 baris gap dihilangkan. Renderer menerbitkan `text_area` tersebut ke `Geometry`, sehingga cursor, mouse, page/scroll dan viewport mengikuti offset yang sama. Klik gap hanya memfokuskan panel, tidak memindahkan cursor dokumen.
- Preflight split memakai body compact maksimum, bukan menganggap dekorasi normal wajib. Split baru dapat menurunkan chrome global menjadi compact agar seluruh panel tetap terlihat. Collapse sementara tidak mengubah tree/ratio.
- Border biasa hanya memfokuskan panel; baris border bawah subtree atas menjadi hit resize Rows. Tab/gutter/teks memiliki hit masing-masing. Gutter tetap memetakan baris melalui layout teks. Wheel memakai kartu di bawah pointer tanpa mengganti fokus; page/scroll memakai tinggi inner text, bukan outer frame.
- Gap explorer dan split Columns tetap satu kolom. Pada Rows, `PaneRect` tidak overlap: panel bawah dimulai tepat setelah border bawah panel atas, tanpa baris kosong tambahan. Minimum tinggi adalah `h1 + h2`; span anak berjumlah `parent.height`. `Divider.area` hanya menumpang border bawah subtree atas; resize memakai offset satu baris dari koordinat hit ke span pertama.
- Renderer divider split hanya menghias gap Columns; Rows tidak menulis glyph tambahan pada border atau celah. `Divider.area` Rows tetap diterbitkan untuk input meskipun indikator visual dihapus. Frame tidak disatukan atau diberi junction antar-panel; judul memakai `Block::title` biasa, tanpa clipping khusus shared border.
- `theme.canvas` adalah background default terminal (`Color::Reset`) di semua palet, bukan RGB solid; token `surface`/`raised` tetap tidak ada. Tab aktif, seleksi dan prefix field fokus memakai pasangan `selection`/`fg` yang sama agar ikon, badge dan close tidak kehilangan kontras. Ikon chrome/kategori memakai slot profil tetap dan modifier bold; Nerd memakai Material Design solid. Judul memakai span pinjaman dan helper memakai tabel keycap/label/KeyEvent statis, bukan string gabungan setiap frame. Font tidak dipasang aplikasi.

### Scrollbar editor

- `chrome::pane_areas` mencadangkan satu row horizontal dan satu kolom vertikal di dalam frame, independen dari overflow agar geometri tidak berubah saat scroll. Horizontal hanya jika body menyisakan minimal satu row teks; vertikal hanya jika tinggi teks minimal dua row. Setiap track harus memiliki minimal dua sel sepanjang sumbunya setelah alokasi sumbu lain; jika tidak muat, track disembunyikan, bukan menerbitkan thumb nol sel. Minimum panel tetap; track tidak menimpa tab, gutter, teks, border atau divider.
- Modul `scrollbar` menghitung thumb, page dan pemetaan drag dalam sel terminal. Batas vertikal adalah `line_count - text_height` yang disaturasi; wheel/autoscroll memakai batas yang sama. Batas horizontal memakai maksimum lebar render baris yang terlihat ditambah satu sel caret EOL, bukan byte/scalar atau lebar seluruh dokumen.
- `Tab.horizontal_width` menyimpan pekerjaan turunan per view dengan key revisi teks/top/tinggi viewport. Perubahan lebar memakai hasil yang sama tetapi tetap menormalisasi `left_cell` terhadap lebar teks baru, termasuk pane nonaktif. `LineWidthCursor` melanjutkan posisi scalar/byte/kolom pada boundary grapheme memakai scanner yang sama dengan renderer; pengukuran tidak membuat string escape tampilan atau menyalin dokumen. Cache bukan history/snapshot dan dikosongkan pada reload, restore dan split baru.
- Aplikasi mengerjakan pengukuran round-robin pada pane terlihat dalam budget kooperatif dua milidetik per update; pekerjaan pending memakai idle tick 16 ms. Budget bukan preemption atau SLA untuk satu grapheme besar. Renderer menerima metadata hasil, menolak extent yang key-nya stale, dan menampilkan `?` pada track horizontal sebelum hasil tersedia; tidak menerbitkan thumb/hit palsu.
- Klik track melakukan page scroll. Press/no-motion mempertahankan offset awal; setelah gerakan pertama, drag tetap terikat skala awal tetapi kembali ke sel endpoint meng-clamp tepat ke 0/maksimum, termasuk thumb yang terkuantisasi. Scrollbar hanya mengubah viewport pane asal, tanpa fokus, cursor, anchor, preferred column, teks, dirty atau history. Capture memeriksa view ID/revisi/text rect; perubahan konteks membatalkan drag stale. Klik setelah viewport bergerak menunggu redraw.
- Rail memakai token muted dan thumb fg/accent + bold, semuanya dengan background terminal `Reset`. ASCII memakai `-`/`|`/`#`, profil Unicode/Nerd memakai garis tipis/tebal. Track non-overflow tidak interaktif; scrollbar explorer, help dan field tidak termasuk kontrak ini.

### Scrollbar Explorer dan F1

- `chrome::scroll_rows` mencadangkan satu kolom di kanan konten bila panjang rail minimal dua sel dan masih tersisa kolom teks. `SidebarAreas.rows`/`Geometry.sidebar` mengecualikan rail; parent row dan divider Explorer tetap terpisah. Sidebar satu row tidak menerbitkan rail atau thumb nol.
- `HelpLayout.catalog` mengecualikan rail sebelum wrapping. Draw, page dan limit memakai lebar katalog yang sama; footer Esc/F1 tetap memakai lebar inner penuh. Jumlah row katalog statis dimemoisasi per lebar runtime dengan slot `OnceLock`; perubahan scroll tidak menghitung ulang seluruh katalog atau menyalinnya ke heap.
- `Geometry.sidebar_scrollbar` dan `help_scrollbar` membawa hit yang benar-benar digambar. Field bantuan dihapus ketika modal berganti/tutup atau viewport terlalu kecil; metadata sidebar dihapus ketika panel tersembunyi. `help::draw` mengembalikan geometri scrollbar ke caller, bukan mengubah state aplikasi.
- `ScrollbarTarget` menyatukan capture editor, Explorer dan Help dengan satu jalur start/drag/scroll. Selain guard editor yang sudah ada, capture memeriksa jumlah row/visibility Explorer atau viewport/modal Help, lalu track dan maksimum yang masih sama. Semua key membatalkan capture scrollbar; drag milik modal tidak diteruskan ke editor/Explorer di belakangnya.
- Explorer hanya mengubah `top`, bukan pilihan entry, root, fokus, panel tujuan atau dokumen. Help hanya mengubah offset katalog dan tidak menyentuh modal yang ditangguhkan, nilai/seleksi field atau fokus tombol. Navigasi keyboard dan resize tetap boleh mengikuti pilihan/caret seperti sebelumnya.
- Page, wheel dan drag memakai rentang penuh viewport; press/no-motion dan endpoint terkuantisasi mengikuti primitive editor. Rendering memakai rail/thumb dan token background `Reset` yang sama, tanpa konvensi warna atau algoritme drag kedua. Tidak menambahkan scrollbar horizontal Explorer, indikator overflow field, hover atau runtime switcher.




## Transisi browser folder

- `Explorer` menormalisasi root satu kali pada konstruksi. `go_parent` dan `enter_selected_directory` membangun listing pengganti sebelum commit root/rows/selection/top. Parent memilih folder sebelumnya; error mempertahankan state usable dan `/` adalah no-op.
- Enter ke folder menggunakan pembukaan komponen `NOFOLLOW` dari root saat ini, bukan canonicalize path anak yang dapat mengikuti symlink pengganti. Klik folder tetap toggle tree; Enter/helper Open melakukan re-root.
- Wrapper `Workspace` menyinkronkan `explorer_root`/`root_label` hanya setelah sukses, termasuk retry. Dokumen/history/path, pane/tree dan CWD proses tidak berubah. Browser bukan sandbox dan belum memiliki history Back/Forward.
- Baris parent tidak masuk hit/scroll/page tree. Klik parent ditangani sebelum baris biasa dan tidak diteruskan sebagai Backspace ke dokumen; stale guard tetap berlaku.

## Grammar bundling dan profil interaktif

- `build.rs` menggabungkan grammar default newline dengan TypeScript/TSX dari konversi bat, memeriksa context link dan menghasilkan dump uncompressed. `syntax::load()` mendeserialisasikannya satu kali melalui `LazyLock`; setiap Highlighter meminjam data grammar/regex immutable yang sama.
- `build_support/markdown.rs` memberi necessary-pipe lookahead pada dua ekspansi aturan tabel Markdown. Aturan/capture/context asli dipertahankan; paragraf tanpa pipe tidak perlu menjalankan balanced-cell table lookahead. Validasi template/konteks/cakupan menggagalkan build bila grammar berubah dan invariant perlu ditinjau. Tidak ada pemuatan grammar atau alokasi guard baru pada runtime; hasil ekuivalensi scope dan pengukuran ada di [ROADMAP.md](ROADMAP.md#hasil-optimasi-lookahead-tabel-markdown).
- Scope markup tidak dibuang sebagai `Plain`: highlighter memetakan heading, emphasis, raw code, link/image, quote dan marker list/separator ke token tersendiri. `Theme::syntax` memakai palet semantik yang sudah ada; renderer dan prioritas seleksi tetap. Aturan ini hanya diklasifikasikan saat capture token viewport, bukan ketika mengejar state prefix yang tidak terlihat.
- Pin sumber dan MIT notice berada di `assets/syntaxes/`; runtime tidak membaca YAML grammar dari disk/proyek atau jaringan. Grammar asli menangani type/generic/template dan JSX beserta embedded expression/comment, bukan alias JavaScript. Ini bukan type checker/LSP atau jaminan seluruh sintaks TypeScript terbaru.
- Backend regex-fancy diganti sepenuhnya oleh Oniguruma. Tradeoff native/compiler C dan notice distribusi dinyatakan pada dependency di atas. Tidak ditambah thread parser, salinan seluruh dokumen, atau special-case mematikan highlight Markdown.
- Profil `dev` memakai opt-level 1, mempertahankan debug information/assertions; `test` eksplisit opt-level 0. Ini mengurangi biaya hitungan sel/render pada `cargo run` tanpa memaksa pengguna beralih ke release; kompilasi dev pertama lebih mahal.
- Hasil perbandingan input tetap, CPU proses dan PTY tercatat di [ROADMAP.md](ROADMAP.md). Gap vertikal luar dihapus tanpa menghilangkan frame/minimum editor.

## Keputusan session restore otomatis

Disetujui pengguna dan tersedia; bukti fungsional Linux/tmux dicatat di [ROADMAP.md](ROADMAP.md#hasil-session-restore-otomatis). Verifikasi keamanan terpisah tidak dianggap lolos hanya karena build/TUI berhasil.

- `naf` tanpa path memulihkan sesi pengguna dan menyimpannya saat Ctrl+Q. Startup tanpa path selalu menampilkan explorer, mengalahkan visibility snapshot lama tanpa mengubah root/lebar atau state editing. `--no-session` menonaktifkan seluruh pembacaan/penulisan snapshot; tanpa path explorer juga tampil. Launch file/folder tetap terpisah dan tidak mengubah sesi default: file memulai dengan explorer tersembunyi, folder menampilkannya. Ctrl+B men-toggle visibility selama aplikasi berjalan; format snapshot tidak berubah.
- Snapshot menyimpan teks satu kali per dokumen, baseline disk asli, format/read-only/dirty, tab per panel, view lokal, active tab/panel, tree/ratio split serta root/visibility/fokus/lebar explorer. Undo/redo dimulai ulang setelah restore; dokumen shared tetap satu buffer/history. Buffer dipulihkan persis, bukan di-reload otomatis dari disk; baseline asli menjaga dialog konflik saat Save jika sumber berubah/hilang.
- Modul `session` memiliki I/O snapshot dan lock; workspace memiliki ekspor/impor serta invariant state; layout memiliki validasi topologi. Rendering tidak melakukan I/O sesi. Teks Rope dialirkan per chunk, bukan dikumpulkan menjadi String seluruh dokumen.
- Lokasi default `$XDG_STATE_HOME/notefart/session.naf`, atau `$HOME/.local/state/notefart/session.naf`; base harus absolut. Direktori privat 0700, file 0600 dan exclusive nonblocking lock mencegah dua sesi default saling menimpa. Snapshot bukan enkripsi atau secure erase.
- Format v1: magic/version, panjang metadata JSON berbatas, payload UTF-8 per dokumen dan checksum BLAKE3. Path disimpan sebagai byte Unix agar nama non-UTF-8 tetap utuh. Simpan melalui tempfile satu direktori, fsync, atomic replacement dan directory sync. Snapshot rusak/versi asing gagal jelas sebelum TUI tanpa diganti sesi kosong; `--no-session` tetap dapat dipakai.
- Batas snapshot: metadata 4 MiB, teks 64 MiB/dokumen, total file 256 MiB, 1024 dokumen, 256 panel dan 4096 view. Penolakan simpan mempertahankan editor/buffer; bukan pemotongan diam-diam atau janji CPU/RSS. Error sebelum persist mempertahankan snapshot lama; sesudah replace, kegagalan directory sync berarti durability tidak pasti dan editor tetap terbuka.
- Serde 1.0.229 (sudah transitif) menjadi dependency langsung dan serde_json 1.0.145 ditambahkan untuk metadata bertipe; versi dipin ke versi yang diverifikasi, bukan klaim terbaru. Keduanya MIT OR Apache-2.0 dengan MSRV 1.56/1.61, sesuai toolchain terpin. Teks tidak diserialisasi sebagai JSON. Metadata resmi: [serde](https://crates.io/api/v1/crates/serde/1.0.229), [serde_json](https://crates.io/api/v1/crates/serde_json/1.0.145).
- Lockfile juga menambahkan memchr 2.8.3 (Unlicense OR MIT, MSRV 1.61), transitif melalui serde_json; [metadata resmi](https://crates.io/api/v1/crates/memchr/2.8.3). Audit dependency historis sebelum perubahan lockfile ini tidak dianggap mencakup dependency baru.


## Kontrak perbaikan AGY P0

- Status foreground membawa level eksplisit Info/Success/Warning/Error; renderer tidak menebak severity dari prefix string. Warning highlight disimpan pada cache dokumen dan dibaca sebagai note terpisah, bukan menimpa hasil aksi pengguna pada idle tick.
- Input path, caret/selection dan continuation Save/Close/Quit dipertahankan ketika Open/Save As gagal agar path dapat diperbaiki dan dicoba lagi. Overwrite/Conflict tetap transisi eksplisit, bukan bypass guard.
- Pesan dan aksi Choices memakai perhitungan fit yang sama pada renderer/controller. Jika seluruh warning dan tombol tidak muat, hanya Resize/Esc yang aktif; mnemonic destruktif tidak boleh bekerja secara tersembunyi.
- Discard tetap berlaku segera per dokumen. Cancel pada konfirmasi berikutnya tidak membatalkan keputusan sebelumnya; pesan harus menyatakan hal ini. Ini bukan transaksi discard massal baru.
- Legacy Alt+] dan pembuka OSC memakai byte yang sama. Tidak menambah timeout yang menjadikan payload control string belum selesai sebagai shortcut. Decoder mempertahankan drain BEL/ST; bila input tertahan setelah idle, tampilkan hint pemulihan Ctrl+G (BEL) dan bersihkan hint setelah parser aman kembali. Pengujian jalur keamanan dikerjakan model ATMO pilihan pengguna.
- Startup explorer yang telah dipilih pengguna tetap: tanpa path/folder tampil, argumen file hidden, Ctrl+B toggle; visibility snapshot tidak mengalahkan kebijakan startup. Saran fitur tambahan AGY tetap draft, bukan perluasan scope batch ini.
