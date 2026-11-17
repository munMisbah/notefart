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
- Session restore otomatis tersedia pada `naf` tanpa path, termasuk buffer Untitled dan perubahan belum disimpan. Ini bukan autosave/crash recovery; detail di [ROADMAP.md](ROADMAP.md#fitur-tambahan--session-restore).

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
| [E-TO-E_TEST_AGY.md](E-TO-E_TEST_AGY.md) | Audit UI/UX AGY dan status tindak lanjut; usulan tidak otomatis menjadi kontrak produk |
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

`naf` tanpa argumen memulihkan sesi terakhir dari exit normal, atau membuka editor kosong jika belum ada snapshot; explorer mulai tampil meskipun disembunyikan pada sesi sebelumnya. `naf test.txt` langsung membuka file relatif terhadap direktori kerja dengan explorer tersembunyi, tanpa mengubah sesi default. Path yang belum ada baru ditulis saat Save. Tidak perlu subcommand `run`; folder juga diterima sebagai argumen dan menampilkan explorer. Gunakan `naf --no-session` untuk editor tanpa snapshot.

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

Tanpa snapshot, launch tanpa argumen membuka tab kosong dan explorer direktori kerja; restore mempertahankan workspace dengan explorer tampil saat startup. Argumen folder menampilkan explorer dengan root tersebut; argumen file menyembunyikannya dengan induk file sebagai root. Keduanya tidak membaca/menulis sesi default. Ctrl+B menampilkan/menyembunyikan explorer; ukuran hasil drag tetap. File yang belum ada baru dibuat saat Save. `--help` menampilkan bantuan. Stdin/stdout harus TTY. Dukungan awal: Linux, UTF-8 dengan LF/CRLF dan BOM opsional; batas file load/save 64 MiB. Mixed newline/lone CR read-only; invalid UTF-8, NUL dan objek non-regular ditolak.

| Kontrol editor | Aksi |
| --- | --- |
| F1 | Buka bantuan; Esc/F1 di bantuan kembali ke editor, explorer, atau dialog sebelumnya |
| Ctrl+S / F4 | Save / Save As |
| Ctrl+O / Ctrl+N | Open path / tab baru pada panel tujuan terakhir |
| Ctrl+W / Ctrl+Q | Tutup view dengan dirty guard pada view terakhir / simpan sesi dan keluar; launch terpisah memakai dirty guard saat quit |
| Ctrl+Z / Ctrl+Y | Undo / redo shared pada dokumen; posisi seluruh view dipetakan ulang |
| Ctrl+A / Shift+panah | Select all / perluas seleksi |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy/cut/paste melalui backend clipboard yang dipilih; aksi hanya mengenai editor atau field yang fokus |
| Panah, Home/End, PageUp/Down | Navigasi; Ctrl+panah dan Ctrl+Home/End juga tersedia |
| Klik / double-click / drag / wheel | Cursor / pilih kata / seleksi dengan autoscroll / scroll |
| Tab di editor | Spasi ke tab stop 4; seleksi multibaris mengindentasi setiap baris |
| Shift+Tab di editor | Dedent baris aktif/terpilih: satu tab awal atau hingga empat spasi |
| Backspace / Delete | Hapus grapheme sebelumnya / berikutnya |
| Esc pada dialog / Ctrl+U pada field | Cancel / kosongkan field |
| Tab / Shift+Tab / panah pada konfirmasi | Pindah fokus tombol; Enter/Space menjalankan pilihan, fokus awal Cancel; mnemonic tetap tersedia |
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
| Alt+Shift+Left/Right / Up/Down | Saat editor fokus, geser split terdekat dua kolom / satu baris; saat explorer fokus, Left/Right mengubah lebarnya dua kolom. Divider juga dapat di-drag |

Ctrl+H tetap Backspace karena byte terminalnya sama; Replace memakai **Ctrl+R**. Field path, pencarian, dan penggantian maksimum 4096 byte UTF-8 setelah normalisasi/replacement seleksi. Hasil aktif memakai seleksi editor, bukan penandaan semua hasil sekaligus. Pencarian tidak memakai regex atau case folding.

Ctrl+F/Ctrl+R dari editor mengisi query dari seleksi satu baris hingga 4096 byte UTF-8. Seleksi multiline atau terlalu panjang tidak dipotong dan tidak mengganti query lama; pergantian mode di dalam dialog mempertahankan query. Find mengunjungi match overlapping pada kedua arah; Replace All tetap mengganti match non-overlap.

Penghitung Find menampilkan posisi dan total tepat hingga prefix 100 hasil; `100+` berarti match tambahan telah ditemukan, dan posisi di luar prefix ditulis `>100`. Navigasi tetap dapat menjangkau seluruh hasil. `(wrapped)` menandai perpindahan melewati ujung dokumen. Cache mengikuti dokumen/revisi/query dan diinvalidasi pada reload; tidak memindai dokumen setiap frame atau keystroke.

Helper editor memprioritaskan Quit, Files dan Pane pada 80 kolom; `[F1] All` menunjukkan katalog lengkap ketika aksi lain tidak muat. Panah overflow tab yang muted tidak dapat diklik pada batas; shortcut pindah tab tetap wrap. Konfirmasi menampilkan path target atau Untitled, mempertahankan fokus saat kembali dari F1, dan menolak aksi bila konteks/pesan/tombol tidak seluruhnya muat.

Indent/dedent blok mempertahankan arah seleksi dan memetakan cursor/anchor seluruh shared view. Endpoint seleksi tepat pada awal baris berikutnya tidak mengikutsertakan baris itu; baris kosong yang tersentuh tetap diindentasi. Satu perintah menjadi satu undo, termasuk pengembalian tab/spasi asli. Dedent tanpa prefix tidak membuat dirty atau membuang redo; spasi lalu tab hanya menghapus spasinya dan boundary grapheme tetap dijaga. Tab dengan seleksi satu baris masih mengganti seleksi ke tab stop seperti sebelumnya. Shortcut modal/field/explorer tetap memakai konteksnya sendiri.

Explorer membaca direktori secara lazy, folder dahulu, termasuk dotfiles; maksimum 10.000 entri/direktori dengan error eksplisit. **Enter** atau helper **[Enter] Open** masuk folder sebagai root browsing baru; klik folder tetap toggle tree, Left/Right collapse/expand. **Backspace**, helper **[Bksp] Up** atau klik **`..`** naik dan memilih folder sebelumnya. Header menunjukkan path browsing aktual; root awal dinormalisasi/canonical sehingga symlink root eksplisit menampilkan target fisiknya. Kegagalan listing mempertahankan browser sebelumnya, tanpa mengubah CWD proses atau dokumen terbuka. Symlink direktori tidak ditelusuri dari tree; belum ada history Back/Forward maupun rename/delete file.

Klik file membuka tab di panel tujuan terakhir. Alias file memakai registry dokumen yang sama. Wheel bekerja di seluruh kartu termasuk baris parent/border, tanpa mengganti fokus; drag seleksi tetap milik panel asal. Border biasa hanya memfokuskan panel, gutter tetap menavigasi baris. Kiri–kanan berjarak satu kolom dengan grip; atas–bawah tetap dua frame terpisah tanpa baris kosong tambahan. Pemisah horizontal tidak memiliki indikator; drag resize tetap melalui border bawah subtree atas dan shortcut tetap tersedia. Explorer 18–40 kolom termasuk frame; baris parent terpisah dari scroll/page tree dan gap satu kolom dapat di-drag.

Opsi runtime: `--theme dark|light|terminal`, `--color auto|truecolor|256|16|none`, `--icons unicode|nerd|ascii`, dan `--clipboard auto|internal|system`. Auto warna menghormati `NO_COLOR` nonkosong/`TERM=dumb`, lalu `COLORTERM`, lalu `TERM`; override eksplisit menang. Bawaan tanpa konfigurasi: Dark/Auto/Unicode dan clipboard Auto. ASCII mengganti struktur UI, bukan teks/nama file Unicode. Nerd opt-in; pilih sekali melalui konfigurasi, bukan deteksi font. `--probe` tidak menerima opsi editor.

Unicode default memakai ikon grafis bold tanpa font khusus. Profil Nerd memakai **Material Design solid**; atur terminal ke **Nerd Font Mono** terkini (direkomendasikan v3+). Bentuk/ketebalan tetap bergantung terminal/font; aplikasi tidak memasang atau mengganti font. ASCII mempertahankan fungsi dengan marker teks dan frame `+`, `-`, `|`.

Background canvas mengikuti default terminal pada semua tema, termasuk terminal glass/transparan; aplikasi tidak mengatur opacity atau blur. Hanya tab aktif, seleksi dan fokus kontrol yang memakai highlight. Tab aktif mempertahankan marker serta bold dan memakai foreground yang terbaca untuk ikon, dirty/read-only dan close. Ada satu baris kosong antara tab dan isi editor; gap dipadatkan pada tinggi panel minimum 4 baris agar editor 12×6 tetap dapat dipakai. Pilih Dark/Light sesuai latar terminal, atau Terminal untuk foreground default.

Field mendukung panah/Home/End/Ctrl+panah, Shift+movement, Ctrl+A, Backspace/Delete, serta klik/drag pada boundary grapheme. Ctrl+C/X/V memakai backend clipboard terpilih dan hanya mengenai field yang fokus; Ctrl+Z/Y dalam dialog tidak mengubah dokumen belakang. Path menolak karakter kontrol; paste Find/Replace menormalisasi CRLF/CR menjadi LF. Input invalid/oversized ditolak utuh tanpa membuang seleksi. CLI tetap mendukung path non-UTF-8; Save As menerima input UTF-8.

F1 menangguhkan dialog, bukan membatalkannya: isi, caret, seleksi dan pending Save/Close/Quit dipertahankan. Bantuan memakai katalog yang sama dengan `--help`; panah, PageUp/Down, Home/End dan wheel menggulir baris visual. Tombol helper/dialog yang terlihat dapat diklik.

Save As ke target existing meminta konfirmasi, termasuk path dokumen sendiri meski memiliki beberapa view. Target milik **dokumen lain** ditolak. Save/path/dirty/read-only dipakai bersama semua view. Konflik menawarkan overwrite, Save As, reload/discard lokal atau cancel; reload mereset seluruh view dokumen. Open/Save As yang gagal mempertahankan path, caret/selection dan pending Save/Close/Quit agar dapat diperbaiki lalu dicoba lagi; close/quit baru berlanjut setelah Save sukses. Esc membatalkan continuation. Discard langsung membuang dokumen yang sedang dikonfirmasi, bukan Discard All; Cancel berikutnya tidak mengembalikan discard sebelumnya.

Menutup satu view dirty tanpa prompt jika view lain masih ada. Tab terakhir pada panel non-terakhir menutup panel tersebut; panel terakhir mendapat blank baru. Close panel memeriksa semua dokumen unik yang kehilangan view terakhir; quit memeriksa setiap dokumen dirty sekali, bukan sekali per view.

Minimum editor tunggal compact **12×6 sel**; minimum tiap panel lebar max(12, digit gutter + 5), tinggi 4 baris termasuk border, tab dan satu baris teks. Header/dock penuh memerlukan lebar ≥40, tinggi ≥18 dan seluruh tree masih muat setelah dekorasi. Jika tidak, chrome global dipadatkan dahulu. Split mengikuti minimum subtree dan ditolak jika tree baru tidak muat bahkan dengan chrome compact tanpa explorer. Saat resize, explorer disembunyikan jika perlu; bila tree tetap tidak muat hanya panel aktif ditampilkan sementara. F10 tetap menjangkau panel tersembunyi; tree/ratio/view pulih saat diperbesar. Dialog umum minimum 20×6, Find/Replace 40×10; aksi tersembunyi ditolak dan Esc membatalkan. Bantuan dapat di-scroll pada 20×6; di bawahnya tersedia petunjuk resize dan kembali dengan Esc/F1.

Batas 20×6 merupakan minimum dasar dialog, bukan jaminan semua konfirmasi muat. Seluruh pesan dan tombol Choices harus terlihat sebelum mnemonic atau klik diaktifkan; jika tidak, dialog menampilkan Resize/Esc. Notice editor yang terlalu kecil memprioritaskan `Ctrl+Q` atau `^Q` agar instruksi keluar tidak hilang karena elisi.

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

## Session restore otomatis

- `naf` tanpa path memulihkan tab/urutan, panel aktif, split/ratio, cursor/selection/viewport, root/fokus/lebar explorer, dan teks shared termasuk Untitled/dirty. Explorer selalu mulai tampil meskipun snapshot terakhir menyembunyikannya; lebar tersimpan tetap dipakai. Ctrl+Q menyimpan sesi, **bukan file sumber**, lalu keluar hanya setelah simpan berhasil.
- `naf --no-session` serta `naf FILE/FOLDER` tidak membaca, menulis, atau menghapus snapshot default. Ctrl+Q pada launch ini tetap menawarkan Save/Discard/Cancel. `--no-config` hanya mengabaikan preferensi dan tidak menonaktifkan sesi.
- Lokasi: `$XDG_STATE_HOME/notefart/session.naf`, atau `$HOME/.local/state/notefart/session.naf`; base harus absolut. Direktori privat 0700 dan file 0600, tetapi **tidak dienkripsi**. Snapshot dapat berisi rahasia; gunakan mode terpisah sebelum mengedit data yang tidak boleh masuk snapshot.
- Satu sesi default memegang lock eksklusif. Untuk instance tambahan, gunakan `--no-session` atau buka file/folder secara eksplisit. Snapshot rusak/versi asing gagal startup tanpa diganti sesi kosong; `--no-session` tetap membuka editor terpisah.
- Restore memulihkan buffer persis, tidak membaca ulang atau menimpa file sumber. Baseline asli dipertahankan agar Save eksplisit tetap mendeteksi sumber berubah/hilang. Undo/redo dimulai ulang setelah restore; dirty state tidak dibersihkan.
- Close tab/panel tetap memakai Save/Discard/Cancel. Buffer yang di-Discard tidak masuk snapshot normal-exit berikutnya. Tidak ada autosave, simpan saat signal/panic, atau crash recovery; exit abnormal meninggalkan snapshot lama, bukan perubahan sesi berjalan.
- Batas snapshot: metadata 4 MiB, teks 64 MiB/dokumen, total file 256 MiB, 1024 dokumen, 256 panel dan 4096 view. Simpan ditolak tanpa memotong buffer dan editor tetap terbuka. Kegagalan sync sesudah replace dilaporkan sebagai ketidakpastian durability, bukan klaim disk tak berubah.

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

Status memakai level Info/Success/Warning/Error eksplisit, bukan tebakan prefix pesan. Error/warning mendahului path dan metadata opsional; elisi tengah mempertahankan awal aksi dan akhir penyebab tanpa menambah tinggi dock. Warning highlight disimpan per dokumen sebagai note terpisah (atau `[Plain]` saat ruang terbatas), sehingga idle tidak menimpa hasil aksi pengguna. Pengukuran file besar/baris panjang serta batasnya ada di [ROADMAP.md](ROADMAP.md#hasil-status-panjang-dan-profil-dokumen-besar); batas load/save bukan janji semua bentuk dokumen responsif.

Lookahead tabel Markdown kini melewati percobaan balanced-cell pada input tanpa pipe, tanpa menghapus aturan highlight. Pengukuran fixture lokal menunjukkan CPU catch-up 0,80→0,56 detik; nano masih lebih ringan. Ini bukan kuota CPU/RAM atau jaminan seluruh Markdown cepat. Metode, perbandingan nano dan batasnya ada di [ROADMAP.md](ROADMAP.md#hasil-optimasi-lookahead-tabel-markdown).

Warna Markdown kini mencakup heading, bold/italic, inline/raw code, link/image, quote dan marker list; sebelumnya scope markup jatuh ke `Plain` meskipun grammar sudah mengenalinya. Ini syntax coloring teks sumber, bukan preview Markdown atau highlighting bahasa di dalam code fence. Mode tanpa warna tetap dihormati; pemetaan palet ada di [DESIGN.md](DESIGN.md#tema-dan-token-warna).

Baris panjang memakai cache geometri sparse per dokumen sehingga posisi cursor, mouse dan render horizontal tidak berulang kali menelusuri seluruh prefix. Cache mempertahankan fase tab, grapheme Unicode dan escape kontrol serta menginvalidasi suffix pada edit/undo/redo. Lookup dingin dan lookbehind Unicode panjang tetap dapat mahal; hasil TUI dan batas pengukuran di [ROADMAP.md](ROADMAP.md#hasil-geometri-sparse-untuk-baris-panjang).

### Validasi release terbaru

Release `./target/release/naf` kini juga memiliki scrollbar vertikal Explorer dan bantuan F1. Klik track melakukan page scroll, drag thumb menggeser viewport tanpa mengganti pilihan file/fokus atau state modal yang ditangguhkan; rail terpisah dari parent row, divider dan footer. DeepSeek memverifikasi TUI nyata dan **130 kasus tes berbeda** lulus pada empat filter terarah, bukan suite seluruh target. [Bukti, hash dan batas](ROADMAP.md#hasil-scrollbar-explorer-dan-f1).

Scrollbar vertikal/horizontal per view editor tetap tersedia, termasuk pengukuran lebar bertahap dan rail minimum. Batch editor sebelumnya ditutup dengan 88 tes recheck terarah serta TUI nyata setelah perbaikan batas; [bukti editor](ROADMAP.md#hasil-scrollbar-editor).

Indent/dedent blok tetap tersedia; batch sebelumnya mencakup 113 tes terarah dan 23 skenario TUI. [Bukti indentasi](ROADMAP.md#hasil-indent-dan-dedent-blok).

Penutupan tahap 4 tetap berlaku untuk matriks Linux yang disetujui pengguna: PTY, VTE/Xvfb, client tmux dan SSH loopback pada [batch P1 sebelumnya](ROADMAP.md#hasil-p1-aksesibilitas-dan-pencarian). Uji indentasi serta scrollbar editor/Explorer/F1 memakai input/protokol tmux/PTY, bukan desktop atau mouse fisik; publish/installer tetap terpisah. Indikator overflow field dan theme/keymap runtime belum termasuk fitur ini.

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
- Legacy Alt tertentu dapat identik dengan pembuka control string terminal. Bila muncul hint `Ctrl+G resumes input`, tekan Ctrl+G untuk mengakhiri sequence yang belum selesai; input sesudahnya kembali normal. Parser tidak mengubah payload control string menjadi shortcut lewat timeout. Ini bukan shortcut editing atau dukungan otomatis untuk seluruh kombinasi Alt.
- File baru dibuat dengan mode 0600. Replacement mempertahankan mode/uid/gid/xattrs/ACL atau menolak jika tidak bisa menjaganya. Save langsung ke symlink/hard link ditolak; gunakan Save As ke file regular baru. Lihat [SECURITY.md](SECURITY.md).
- Save bersifat sinkron, bukan background; filesystem lambat dapat menahan interaksi. Race final check→rename tidak dieliminasi sepenuhnya.
- Tidak ada autosave/crash recovery. Signal terminasi tidak menyimpan edit; SIGKILL, suspend eksternal, atau hilangnya terminal tidak menjamin restorasi.
- Clipboard sistem membutuhkan `wl-clipboard` pada Wayland atau `xclip` pada X11; aplikasi tidak menginstalnya otomatis. OSC 52 belum tersedia. Paste terminal tetap tersedia dan clipboard internal dapat dipilih eksplisit.
- Editor diuji di PTY Linux, backend terminal Orca langsung, client tmux 3.5a yang benar-benar di-attach, serta koneksi SSH nyata ke sshd loopback terisolasi. Matriks GUI memakai xfce4-terminal/VTE di Xvfb virtual, dengan profil Dark/Light/Terminal/none dan ikon Nerd/Unicode/ASCII. Ini aplikasi/emulator aktual, bukan layar atau input fisik pengguna; SSH non-loopback dan OS lain belum dibuktikan. [Metode dan batas](ROADMAP.md#hasil-p1-aksesibilitas-dan-pencarian).
- Pada tmux dengan prefix bawaan Ctrl+B, tekan **Ctrl+B dua kali** untuk mengirim satu Ctrl+B ke `naf` (hide/unhide explorer). Satu Ctrl+B ditangani tmux; keymap tmux pengguna tidak diubah aplikasi.
- Kontras token Dark/Light truecolor dan xterm-256 diperiksa unit test; canvas mengikuti background terminal. Palet ANSI16/Terminal, wallpaper/transparansi serta fallback glyph/font client tetap dikendalikan pengguna dan tidak menjamin rasio kontras universal.
- Event log diagnostik hanya ada di memori dan menampilkan preview; jangan paste rahasia ketika merekam layar probe. Editor tidak menyimpan log isi dokumen/clipboard ke disk.
- Replace All memeriksa ukuran jendela teks hasil komposisi sebelum alokasi (maksimum 64 MiB), tanpa menyimpan daftar semua match. Batas ini bukan batas total memori dokumen/history.
- Session restore normal-exit tersedia, tetapi crash recovery belum ada. Konfigurasi preferensi dan snapshot buffer/tab terpisah. Penutupan tahap 4 hanya berlaku untuk matriks Linux teruji di atas, bukan kesiapan publish atau dukungan seluruh terminal/host.
