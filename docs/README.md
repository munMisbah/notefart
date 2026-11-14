# Notefart

Editor teks terminal dengan kontrol familiar ala Notepad, tampilan terinspirasi superfile, serta dukungan keyboard dan mouse.

**Status: editor hybrid panel + tab dengan browser folder dan UI bergaya superfile tersedia.** Frame berikon bold/solid, background seragam dan helper berkelompok tetap ada, kini tanpa dua gap vertikal luar. Highlight Markdown memakai backend/cache yang lebih ringan; TypeScript/TSX sudah memiliki grammar asli. Preferensi tema/warna/ikon/clipboard dapat disimpan, dan clipboard sistem lokal Wayland/X11 tersedia jika helper terpasang. Split/shared history, field editable, F1 dan guard file tetap berlaku; produk tetap editor teks, bukan IDE, dan kesiapan rilis belum dikunci final.

## Arah yang disepakati

- Nama: **Notefart**.
- Produk akhir tetap editor teks familiar ala Notepad, **bukan IDE**.
- Bahasa Rust dan UI Ratatui sebagai arah teknologi.
- Kontrol mudah dipakai tanpa mode editing wajib seperti Vim.
- Explorer direktori dengan folder yang dapat diekspansi.
- Syntax highlighting.
- Model hybrid: beberapa panel editor, masing-masing memiliki beberapa tab.
- Mouse sebagai interaksi utama yang didukung, bukan sekadar pelengkap.
- Shortcut helper di bagian bawah.
- Prioritas awal balanced: kenyamanan penggunaan dan fondasi editor dikembangkan bersama.
- Session restore dicatat sebagai fitur tambahan; belum diimplementasikan. Cakupan rencana ada di [ROADMAP.md](ROADMAP.md).

Detail di luar daftar tersebut adalah **usulan**, bukan keputusan final atau janji fitur. Balasan dan gambar dari percakapan lama belum tersedia lengkap; dokumen ini tidak menganggap detail yang tidak terbaca sebagai kesepakatan.

## Dokumen

| Dokumen | Isi |
| --- | --- |
| [AGENTS.md](../AGENTS.md) | Aturan delegator/integrator, worker, reviewer, kontrak tugas, dan verifikasi |
| [PRODUCT.md](PRODUCT.md) | Tujuan, kebutuhan pengguna, batas produk, dan kriteria penerimaan |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Model data, aliran event, komponen, dan kandidat teknologi |
| [DESIGN.md](DESIGN.md) | Layout, fokus, mouse, shortcut, dan perilaku antarmuka |
| [SECURITY.md](SECURITY.md) | Model ancaman, integritas file, terminal, dan clipboard |
| [ROADMAP.md](ROADMAP.md) | Tahapan implementasi dan bukti kelulusan tiap tahap |
| [CHANGELOG.md](../CHANGELOG.md) | Perubahan implementasi yang benar-benar tersedia |

## Cara memakai rencana ini

1. Revisi keputusan produk dan interaksi sebelum implementasi besar.
2. Validasi perilaku terminal melalui spike kecil, terutama input, mouse, Unicode, dan clipboard.
3. Implementasikan bertahap mengikuti roadmap; sebuah tahap selesai hanya setelah perilakunya dibuktikan.

Belum ada jadwal rilis atau dukungan platform final. Dependency batch ini dikunci di Cargo.lock; versi tersebut bukan klaim versi terbaru.

## CI dan artifact Linux

[Workflow Linux CI](../.github/workflows/ci.yml) berjalan pada push, pull request dan manual dispatch setelah source berada di GitHub. Target awal `ubuntu-24.04` x86_64: install compiler C, muat toolchain yang dipin, cek format, jalankan seluruh target tes, build release, lalu smoke CLI `--version`/`--help`.

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo build --locked --release --bin naf
```

Run sukses mengunggah artifact `naf-linux-x86_64-` diikuti commit SHA, disimpan 7 hari. Unduh melalui tab Actions → run yang sukses → Artifacts. Di dalamnya ada `naf-linux-x86_64.tar.gz`, berisi executable `naf`, `config.toml.example` dan notice Oniguruma/Sublime Packages. Ekstrak tar untuk mempertahankan executable bit. Binary GNU/Linux ini bukan static/musl atau jaminan berjalan pada semua distro/glibc; belum ada build Windows/macOS.

Workflow hanya memakai izin `contents: read`, checkout tidak menyimpan credential, Action dipin full commit SHA dan tidak ada auto-publish Release/installer. Privasi artifact mengikuti visibilitas repository, **bukan** nama workflow: buat repository **Private** sebelum push jika source harus tertutup. Jangan memasukkan credential atau config pribadi; `.gitignore` mengecualikan root `config.toml`, `.env`/`.env.*` dan `target/`, tetapi tidak melindungi file rahasia yang sudah tracked. Template tetap dapat di-commit.

Repository tujuan: [munMisbah/notefart](https://github.com/munMisbah/notefart). Definisi/command CI telah diperiksa lokal; status eksekusi GitHub-hosted runner dan artifact tersedia di [Actions](https://github.com/munMisbah/notefart/actions) setelah push. Validasi lokal bukan klaim run GitHub sukses. Aktivasi Actions/izin akses serta kuota private mengikuti pengaturan akun/repo GitHub. Installer/setup konfigurasi tetap ditunda sampai publish.

Pin diverifikasi dari sumber resmi: [Rust 1.99.1](https://static.rust-lang.org/dist/channel-rust-1.99.1.toml), [checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1), dan [upload-artifact v7.0.2](https://github.com/actions/upload-artifact/releases/tag/v7.0.2). Pembaruan pin harus ditinjau, bukan mengikuti tag bergerak secara otomatis.

## Menjalankan editor

Jalankan dari root repository dengan Rust/Cargo edition 2024 dan compiler C untuk backend Oniguruma native. `rust-toolchain.toml` mem-pin Rust/Cargo 1.99.1 beserta rustfmt; rustup mengunduh toolchain itu jika belum tersedia, tanpa mengganti default global. Grammar di-embed saat build; editor tidak perlu jaringan atau toolchain TypeScript saat membuka file.

Nama aplikasi tetap **Notefart**, nama executable **`naf`**. Jika binary sudah berada di `PATH`:

```sh
naf
naf test.txt
```

`naf` tanpa argumen membuka editor kosong; `naf test.txt` langsung membuka file relatif terhadap direktori kerja. Path yang belum ada disiapkan sebagai dokumen baru dan baru ditulis saat Save. Tidak perlu subcommand `run` atau flag khusus. Folder juga tetap diterima sebagai argumen.

Build lokal menghasilkan `target/debug/naf` atau `target/release/naf`; tanpa instalasi ke PATH, jalankan misalnya `./target/debug/naf test.txt`. `cargo run` otomatis memilih target `naf`. Belum ada pemasangan otomatis ke `~/.local/bin` atau `/usr/local/bin`; installer tetap ditunda. Nama package Cargo dan lokasi preferensi `notefart/config.toml` tidak diubah, sehingga konfigurasi tersimpan tetap dipakai.

Default `cargo run` memakai profil dev opt-level 1 dengan debug information/assertions; profil test tetap opt-level 0. Build dev pertama lebih lama, tetapi hitungan sel/render lebih ringan. `cargo run --release -- …` tetap tersedia untuk penggunaan release.

```sh
cargo run
cargo run -- notes.txt
cargo run -- ./folder-project
cargo run -- --theme light --color 256 --icons ascii ./folder-project
cargo run -- --icons nerd ./folder-project
cargo run -- -- ./nama-file
```

Tanpa argumen membuka tab kosong dan explorer direktori kerja. Argumen folder menentukan root explorer; argumen file membukanya dalam tab, memakai induknya sebagai root. File yang belum ada baru dibuat saat Save. `--help` menampilkan bantuan. Stdin/stdout harus TTY. Dukungan awal: Linux, UTF-8 dengan LF/CRLF dan BOM opsional; batas file load/save 64 MiB. Mixed newline/lone CR read-only; invalid UTF-8, NUL dan objek non-regular ditolak.

| Kontrol editor | Aksi |
| --- | --- |
| F1 | Buka bantuan; Esc/F1 di bantuan kembali ke editor, explorer, atau dialog sebelumnya |
| Ctrl+S / F4 | Save / Save As |
| Ctrl+O / Ctrl+N | Open path / tab baru pada panel tujuan terakhir |
| Ctrl+W / Ctrl+Q | Tutup view aktif / keluar; dirty guard saat dokumen kehilangan view terakhir atau aplikasi keluar |
| Ctrl+Z / Ctrl+Y | Undo / redo shared pada dokumen; posisi seluruh view dipetakan ulang |
| Ctrl+A / Shift+panah | Select all / perluas seleksi |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy/cut/paste melalui backend clipboard yang dipilih; aksi hanya mengenai editor atau field yang fokus |
| Panah, Home/End, PageUp/Down | Navigasi; Ctrl+panah dan Ctrl+Home/End juga tersedia |
| Klik / double-click / drag / wheel | Cursor / pilih kata / seleksi dengan autoscroll / scroll |
| Tab / Backspace / Delete | Indentasi spasi ke tab stop 4 / hapus grapheme |
| Esc pada dialog / Ctrl+U pada field | Cancel / kosongkan field |
| Alt+Left/Right / Ctrl+PageUp/Down | Pindah tab dalam panel aktif; overflow/close dapat diklik |
| Ctrl+B / F2 / Esc | Tampilkan/sembunyikan explorer / pindah fokus / kembali ke editor |
| Panah, Home/End, PageUp/Down di explorer | Navigasi tree; Right/Left expand/collapse, Enter masuk folder atau buka file, F5 refresh |
| Backspace / klik `..` di explorer | Naik ke folder induk; tidak melakukan apa-apa di filesystem root `/` |
| Ctrl+F / Ctrl+R | Find / Replace literal, case-sensitive pada dokumen aktif |
| Enter atau F3 / Shift+F3 | Hasil berikutnya / sebelumnya, wrap; F3 mengulang query setelah dialog ditutup |
| F6 / F7 dalam Replace | Ganti hasil aktif / ganti semua; Replace All satu langkah undo |
| Tab / Shift+Tab di Find/Replace | Pindah field; paste mendukung query dan replacement multiline |
| F8 / F9 | Split kiri-kanan / atas-bawah, membuka view independen dokumen aktif |
| F10 / Shift+F10 | Panel berikutnya / sebelumnya |
| F12 | Tutup panel; periksa setiap dokumen yang kehilangan view terakhir |
| Alt+Shift+Left/Right / Up/Down | Geser divider terdekat dua kolom / satu baris; divider juga dapat di-drag |

Ctrl+H tetap Backspace karena byte terminalnya sama; Replace memakai **Ctrl+R**. Field path, pencarian, dan penggantian maksimum 4096 byte UTF-8 setelah normalisasi/replacement seleksi. Hasil aktif memakai seleksi editor, bukan penandaan semua hasil sekaligus. Pencarian tidak memakai regex atau case folding.

Explorer membaca direktori secara lazy, folder dahulu, termasuk dotfiles; maksimum 10.000 entri/direktori dengan error eksplisit. **Enter** atau helper **[Enter] Open** masuk folder sebagai root browsing baru; klik folder tetap toggle tree, Left/Right collapse/expand. **Backspace**, helper **[Bksp] Up** atau klik **`..`** naik dan memilih folder sebelumnya. Header menunjukkan path browsing aktual; root awal dinormalisasi/canonical sehingga symlink root eksplisit menampilkan target fisiknya. Kegagalan listing mempertahankan browser sebelumnya, tanpa mengubah CWD proses atau dokumen terbuka. Symlink direktori tidak ditelusuri dari tree; belum ada history Back/Forward maupun rename/delete file.

Klik file membuka tab di panel tujuan terakhir. Alias file memakai registry dokumen yang sama. Wheel bekerja di seluruh kartu termasuk baris parent/border, tanpa mengganti fokus; drag seleksi tetap milik panel asal. Border hanya memfokuskan panel, gutter tetap menavigasi baris. Explorer 18–40 kolom termasuk frame; baris parent terpisah dari scroll/page tree dan gap satu sel dapat di-drag.

Opsi runtime: `--theme dark|light|terminal`, `--color auto|truecolor|256|16|none`, `--icons unicode|nerd|ascii`, dan `--clipboard auto|internal|system`. Auto warna menghormati `NO_COLOR` nonkosong/`TERM=dumb`, lalu `COLORTERM`, lalu `TERM`; override eksplisit menang. Bawaan tanpa konfigurasi: Dark/Auto/Unicode dan clipboard Auto. ASCII mengganti struktur UI, bukan teks/nama file Unicode. Nerd opt-in; pilih sekali melalui konfigurasi, bukan deteksi font. `--probe` tidak menerima opsi editor.

Unicode default memakai ikon grafis bold tanpa font khusus. Profil Nerd memakai **Material Design solid**; atur terminal ke **Nerd Font Mono** terkini (direkomendasikan v3+). Bentuk/ketebalan tetap bergantung terminal/font; aplikasi tidak memasang atau mengganti font. ASCII mempertahankan fungsi dengan marker teks dan frame `+`, `-`, `|`.

Field mendukung panah/Home/End/Ctrl+panah, Shift+movement, Ctrl+A, Backspace/Delete, serta klik/drag pada boundary grapheme. Ctrl+C/X/V memakai backend clipboard terpilih dan hanya mengenai field yang fokus; Ctrl+Z/Y dalam dialog tidak mengubah dokumen belakang. Path menolak karakter kontrol; paste Find/Replace menormalisasi CRLF/CR menjadi LF. Input invalid/oversized ditolak utuh tanpa membuang seleksi. CLI tetap mendukung path non-UTF-8; Save As menerima input UTF-8.

F1 menangguhkan dialog, bukan membatalkannya: isi, caret, seleksi dan pending Save/Close/Quit dipertahankan. Bantuan memakai katalog yang sama dengan `--help`; panah, PageUp/Down, Home/End dan wheel menggulir baris visual. Tombol helper/dialog yang terlihat dapat diklik.

Save As ke target existing meminta konfirmasi, termasuk path dokumen sendiri meski memiliki beberapa view. Target milik **dokumen lain** ditolak. Save/path/dirty/read-only dipakai bersama semua view. Konflik menawarkan overwrite, Save As, reload/discard lokal atau cancel; reload mereset seluruh view dokumen. Save gagal/cancel menghentikan close/quit. Discard hanya membuang dokumen yang sedang dikonfirmasi, bukan Discard All.

Menutup satu view dirty tanpa prompt jika view lain masih ada. Tab terakhir pada panel non-terakhir menutup panel tersebut; panel terakhir mendapat blank baru. Close panel memeriksa semua dokumen unik yang kehilangan view terakhir; quit memeriksa setiap dokumen dirty sekali, bukan sekali per view.

Minimum editor tunggal compact **12×6 sel**; minimum tiap panel lebar max(12, digit gutter + 5), tinggi 4 baris termasuk border, tab dan satu baris teks. Header/dock penuh memerlukan lebar ≥40, tinggi ≥18 dan seluruh tree masih muat setelah dekorasi. Jika tidak, chrome global dipadatkan dahulu. Split mengikuti minimum subtree dan ditolak jika tree baru tidak muat bahkan dengan chrome compact tanpa explorer. Saat resize, explorer disembunyikan jika perlu; bila tree tetap tidak muat hanya panel aktif ditampilkan sementara. F10 tetap menjangkau panel tersembunyi; tree/ratio/view pulih saat diperbesar. Dialog umum minimum 20×6, Find/Replace 40×10; aksi tersembunyi ditolak dan Esc membatalkan. Bantuan dapat di-scroll pada 20×6; di bawahnya tersedia petunjuk resize dan kembali dengan Esc/F1.

## Konfigurasi pengguna dan clipboard

Preferensi dibaca dari `$XDG_CONFIG_HOME/notefart/config.toml` jika XDG absolut; selain itu `$HOME/.config/notefart/config.toml`. Tidak membaca konfigurasi dari proyek. Urutan prioritas: **bawaan < konfigurasi pengguna < flag CLI**.

Template berkomentar tersedia di [config.toml.example](../config.toml.example). Coba tanpa mengubah preferensi tersimpan: `cargo run -- --config config.toml.example`. File contoh di folder proyek tidak dibaca otomatis.

Aktifkan Nerd Font sekali dan simpan:

```sh
cargo run -- --icons nerd --save-config
cargo run
```

`--save-config` menyimpan preferensi lalu keluar, tanpa membuka editor. Startup biasa tidak menulis konfigurasi; flag sementara tidak mengubah file tersimpan. Contoh isi:

```toml
theme = "dark"
color = "auto"
icons = "nerd"
clipboard = "auto"
```

- `--config PATH`: pilih file konfigurasi eksplisit; gunakan bersama `--save-config` untuk membuat file baru.
- `--no-config`: abaikan konfigurasi, termasuk untuk memulihkan startup dari file konfigurasi yang rusak.
- File konfigurasi maksimum 64 KiB, UTF-8, hanya empat key string di atas. Salah tipe/key/nilai gagal sebelum raw mode; tidak menjalankan command/plugin. Penyimpanan atomik, bukan penulisan ke file dokumen.
- Clipboard `auto`: Wayland memakai `wl-copy` + `wl-paste` dari paket `wl-clipboard`; X11 memakai `xclip` pada selection CLIPBOARD. Jika backend lokal tidak tersedia atau sesi SSH terdeteksi, gunakan internal dan tampilkan alasannya.
- Clipboard `internal`: tidak mengakses clipboard sistem. `system`: meminta backend eksternal dan menampilkan kegagalan bila tidak tersedia; tidak menyamar sebagai copy sistem yang sukses.
- Tidak ada pembacaan clipboard berkala atau penulisan saat seleksi. Kegagalan cut eksternal mempertahankan teks; kegagalan paste tidak menyisipkan clipboard internal yang lama. Paste eksternal maksimum 1 MiB, UTF-8, ditolak utuh jika tidak valid/terlalu besar; clipboard kosong tidak mengganti seleksi.
- OSC 52 belum tersedia. Di SSH, paste terminal tetap dapat dipakai; `--clipboard system` adalah pilihan eksplisit jika display forwarding memang tersedia.

Batch clipboard/preferensi dibuktikan lewat tes terarah dan **11 sesi PTY**, termasuk probe: X11 dengan Xvfb+xclip, Wayland dengan Sway headless+wl-clipboard, preferensi Nerd tanpa flag, override CLI tanpa menulis file, dialog, shared undo, resize, invalid/oversized/empty paste, helper timeout dan recovery konfigurasi. Seluruh sesi keluar 0 dan memulihkan terminal. Ini bukan klaim matriks emulator/font fisik atau koneksi SSH nyata; rincian di [ROADMAP.md](ROADMAP.md).

## Diagnostik dan evaluasi

```sh
cargo run -- --probe
```

Mode probe hanya mendiagnosis terminal, bukan mengedit file. **Di probe saja**, Ctrl+C keluar; di editor Ctrl+C adalah copy. Probe dan editor memakai decoder input berbatas yang sama.

```sh
cargo run --release --example text_probe
cargo run --release --example highlight_probe
cargo test --bin naf
```

Contoh teks/highlight tetap alat evaluasi terpisah. Produksi memakai Syntect/Oniguruma di luar rendering, checkpoint 128 baris, budget 200 baris dan slice kooperatif 4 ms per dokumen/update. Token render hanya union viewport; cache recent terbatas 512 baris/2 MiB mempercepat scroll balik tanpa menyimpan token gap besar. Edit/path/language/barrier menginvalidasi cache terkait. Baris >16 KiB/error parser menghentikan propagasi dengan warning/plaintext; editing tetap tersedia dan satu operasi regex tidak dapat dipotong paksa.

Batch frame sebelumnya lolos 217 unit test. Batch helper/browser memeriksa 231 jalur unit: 198 di luar Explorer lolos dalam suite dan rerun 33 tes Explorer lolos, termasuk regresi root saat retry. Format dan build sukses. PTY membuktikan helper Open/Save/Up/Help, separator nonaktif, shared save/undo, parent/Enter/empty/root/refresh, recovery permission error, penolakan directory-symlink swap, background seragam, bold ikon, minimum/restore dan empat profil tema/warna/ikon. Rincian di [ROADMAP.md](ROADMAP.md).

Batch performa berikutnya memverifikasi backend/cache Markdown, warna TS/TSX, edit/save/undo shared, loncatan dokumen panjang, hit mouse setelah gap dihapus dan CPU idle. Tes terkait/build serta delapan sesi PTY selesai; benchmark input tetap dan rincian urutan unit ada di [ROADMAP.md](ROADMAP.md). Angka lokal bukan jaminan latency/CPU lintas mesin atau ukuran dokumen.

Status panjang kini memprioritaskan pesan: error clipboard dikenali sebagai error, path mengalah pada pesan, dan error/warning dapat menyembunyikan metadata opsional. Elisi tengah mempertahankan awal aksi dan akhir penyebab tanpa menambah tinggi dock. Pengukuran file besar/baris panjang serta keterbatasan yang ditemukan ada di [ROADMAP.md](ROADMAP.md#hasil-status-panjang-dan-profil-dokumen-besar); batas load/save bukan janji semua bentuk dokumen akan responsif.

Lookahead tabel Markdown kini melewati percobaan balanced-cell pada input tanpa pipe, tanpa menghapus aturan highlight. Pengukuran fixture lokal menunjukkan CPU catch-up 0,80→0,56 detik; nano masih lebih ringan. Ini bukan kuota CPU/RAM atau jaminan seluruh Markdown cepat. Metode, perbandingan nano dan batasnya ada di [ROADMAP.md](ROADMAP.md#hasil-optimasi-lookahead-tabel-markdown).

Warna Markdown kini mencakup heading, bold/italic, inline/raw code, link/image, quote dan marker list; sebelumnya scope markup jatuh ke `Plain` meskipun grammar sudah mengenalinya. Ini syntax coloring teks sumber, bukan preview Markdown atau highlighting bahasa di dalam code fence. Mode tanpa warna tetap dihormati; pemetaan palet ada di [DESIGN.md](DESIGN.md#tema-dan-token-warna).

Baris panjang memakai cache geometri sparse per dokumen sehingga posisi cursor, mouse dan render horizontal tidak berulang kali menelusuri seluruh prefix. Cache mempertahankan fase tab, grapheme Unicode dan escape kontrol serta menginvalidasi suffix pada edit/undo/redo. Lookup dingin dan lookbehind Unicode panjang tetap dapat mahal; hasil TUI dan batas pengukuran di [ROADMAP.md](ROADMAP.md#hasil-geometri-sparse-untuk-baris-panjang).

## Contoh syntax di `testfile`

```sh
cargo run -- --icons nerd testfile
```

| File | Hasil engine yang diperiksa melalui aplikasi |
| --- | --- |
| `config.yaml` | Highlight YAML tersedia |
| `pipeline.yml` | Highlight YAML tersedia |
| `sample.tsx` | Highlight TypeScript + JSX/embedded expression/comment tersedia |
| `sample.ts` | Highlight TypeScript, type/generic/template tersedia |
| `sample.js` | Highlight JavaScript tersedia |
| `index.html` | Highlight HTML tersedia |
| `styles.css` | Highlight CSS tersedia |

File ini fixture visual, bukan frontend yang dipasang/dijalankan Notefart. Grammar TS/TSX asli berlisensi dibundel dari konversi bat, bukan alias ke JavaScript. Ini syntax highlighting, bukan type checking/LSP dan bukan jaminan seluruh sintaks TypeScript terbaru.

## Batas batch ini

- Paste terminal harus memakai bracketed paste. Payload maksimum 1 MiB; oversized/invalid UTF-8 dibuang **utuh**, bukan dipotong atau diinterpretasikan sebagai shortcut. Editor menormalisasi CRLF/CR dari paste menjadi LF. Batas ini tidak membatasi total dokumen, clipboard internal, atau riwayat undo.
- File baru dibuat dengan mode 0600. Replacement mempertahankan mode/uid/gid/xattrs/ACL atau menolak jika tidak bisa menjaganya. Save langsung ke symlink/hard link ditolak; gunakan Save As ke file regular baru. Lihat [SECURITY.md](SECURITY.md).
- Save bersifat sinkron, bukan background; filesystem lambat dapat menahan interaksi. Race final check→rename tidak dieliminasi sepenuhnya.
- Tidak ada autosave/crash recovery. Signal terminasi tidak menyimpan edit; SIGKILL, suspend eksternal, atau hilangnya terminal tidak menjamin restorasi.
- Clipboard sistem membutuhkan `wl-clipboard` pada Wayland atau `xclip` pada X11; aplikasi tidak menginstalnya otomatis. OSC 52 belum tersedia. Paste terminal tetap tersedia dan clipboard internal dapat dipilih eksplisit.
- Editor diuji di PTY Linux TERM=xterm-256color. Probe dengan decoder baru juga diuji di tmux 3.5a terisolasi. SSH, OS lain, glyph/font fisik/Nerd Font, dan matriks emulator belum tervalidasi.
- Tema/profil warna diuji lewat PTY; kontras Dark/Light truecolor dan xterm-256 diperiksa unit test. Palet ANSI16/Terminal milik pengguna tidak menjamin rasio kontras. Glyph/font fisik dan Nerd Font Mono tetap belum divalidasi.
- Event log diagnostik hanya ada di memori dan menampilkan preview; jangan paste rahasia ketika merekam layar probe. Editor tidak menyimpan log isi dokumen/clipboard ke disk.
- Replace All memeriksa ukuran jendela teks hasil komposisi sebelum alokasi (maksimum 64 MiB), tanpa menyimpan daftar semua match. Batas ini bukan batas total memori dokumen/history.
- Session restore dan crash recovery belum tersedia. Konfigurasi hanya menyimpan preferensi, bukan isi buffer/tab. Redesign visual dan polish field tidak menutup tahap kesiapan rilis; validasi penggunaan harian dan matriks terminal lebih luas masih terbuka.
