# Changelog

## Unreleased

### Executable naf

- Ganti satu-satunya target executable menjadi `naf`; nama aplikasi/package Notefart dan namespace konfigurasi `notefart` tetap. Tidak menambahkan alias binary lama.
- `naf` membuka editor kosong dan `naf test.txt` langsung membuka file; path baru dibuat saat Save. Help/version/error prefix, workflow build/artifact dan panduan aktif diselaraskan.
- Format, tiga tes CLI, build dev/release, tiga sesi TUI dan packaging lokal diverifikasi. Binary lama di debug/release/dist dibersihkan; installer/PATH pengguna dan remote GitHub tidak diubah.

### CI Linux dan regresi lengkap

- Tambah GitHub Actions untuk push/PR/manual: format, seluruh target tes, build release, smoke CLI dan artifact Linux x86_64 dengan template serta notice. Toolchain Rust 1.99.1 dipin; Action resmi memakai full SHA, izin read-only dan checkout tanpa credential persisten.
- Artifact berupa tar dengan executable bit, retensi 7 hari, tanpa auto-publish Release atau installer. Ignore config pribadi/.env tanpa mengabaikan template.
- Pemeriksaan lokal: format lolos, **271 tes lolos**, release build dan smoke CLI sukses; actionlint lolos serta packaging/isi/mode binary diverifikasi. Run/upload GitHub belum dilakukan; remote dan repository private belum dibuat/disetel.

### Geometri sparse untuk baris panjang

- Tambah cache geometri turunan per dokumen: maksimum 8 baris panjang, checkpoint sekitar setiap 1024 char dan frontier terakhir. Pencarian kolom/posisi serta render horizontal memulai dari batas grapheme terdekat, bukan selalu awal baris.
- Edit/undo/redo mempertahankan prefix aman dan menginvalidasi suffix; cache baris sesudah edit dibuang dan clone tidak menyalinnya. Fase tab, koordinat char/byte/cell, seleksi dan escape tetap berlaku.
- Perbaiki pairing regional-indicator lintas chunk yang ditemukan saat menguji resumption. Batas sel streaming dibandingkan dengan segmentasi string utuh, bukan mengubah flag menjadi karakter terpisah.
- 16 tes dokumen dan 31 renderer lolos, build serta TUI membuktikan End/edit/undo/mouse, shared save/undo dan resize. Perbandingan sebelum/sesudah beserta batas lookup dingin ada di [ROADMAP.md](docs/ROADMAP.md#hasil-geometri-sparse-untuk-baris-panjang). Tidak menambah soft wrap, session atau installer.

### Perbaikan warna Markdown

- Scope markup kini dipetakan ke token heading, emphasis, code, link, quote dan marker; sebelumnya hanya kategori bahasa pemrograman yang mendapat warna sehingga Markdown tampak plaintext.
- Gunakan palet tema yang sudah ada tanpa mengganti grammar/parser, cache, slice atau renderer. Paragraf/list body tetap biasa, scope nested tetap berlaku dan seleksi tetap mengalahkan warna syntax.
- 19 tes highlighter, 7 tema dan 4 syntax lolos; build serta lima sesi TUI membuktikan warna `AGENTS.md`, scroll balik, edit/undo code fence, seleksi, Dark/truecolor, Light/256, Terminal/16 dan mode none. Batas raw code/Setext dicatat di DESIGN; bukan preview Markdown.

### Optimasi lookahead tabel Markdown

- Tambah necessary-pipe lookahead saat membangun dump grammar: paragraf tanpa pipe tidak menjalankan balanced-cell table lookahead mahal. Aturan/capture/context asli dipertahankan; build menolak perubahan grammar yang perlu peninjauan.
- CPU catch-up EOF pada fixture lokal turun 0,80→0,56 detik; benchmark engine prefix sekitar 24% lebih singkat. Nano masih lebih ringan; tidak ada klaim startup/input/RAM lebih cepat atau batas keras CPU. Metode dan keterbatasan di [ROADMAP.md](docs/ROADMAP.md#hasil-optimasi-lookahead-tabel-markdown).
- Empat tes syntax dan build lolos; TUI membuktikan warna comment, invalidasi setelah edit delimiter, undo, page/wheel dan restorasi terminal. Tidak mengubah engine, long-line policy, session atau installer.

### Status panjang dan pengukuran dokumen besar

- Copy/Cut/Paste failed kini mendapat prioritas/style error. Status yang muat tidak dibatasi setengah baris demi path; error/warning dapat mengalahkan metadata opsional. Pesan berlebih memakai elisi tengah grapheme-safe yang mempertahankan awal aksi dan akhir penyebab, tanpa menambah tinggi dock atau buffer String per frame.
- Perhitungan metadata kolom dilewati jika pesan kritikal pasti mengambil seluruh baris. Helper, hit target, format dokumen dan batas load/save tetap.
- 29 tes renderer, build tanpa warning dan TUI nyata memverifikasi status/warna/profil ikon, clipboard unavailable, warning, Help serta resize. Pengukuran enam bentuk dokumen pada binary akhir beserta RSS/CPU dicatat di [ROADMAP.md](docs/ROADMAP.md#hasil-status-panjang-dan-profil-dokumen-besar).
- Pengukuran menemukan baris tunggal ekstrem masih lambat dan cold jump Markdown masih memakai CPU background. Batch ini tidak mengubah engine dokumen/highlighter atau mengklaim semua file hingga batas ukuran responsif. Session restore tetap backlog; installer ditunda sampai publish.

### Clipboard sistem dan preferensi pengguna

- Tambah clipboard Auto/Internal/System: Wayland melalui wl-clipboard, X11 melalui xclip pada CLIPBOARD; Auto SSH/tanpa helper memakai internal dengan alasan terlihat. Copy/paste editor dan Path/Find/Replace mengikuti fokus; tidak ada akses clipboard saat render/idle atau hanya karena seleksi.
- Copy Wayland memakai MIME UTF-8 eksplisit. Helper tetap/argv terpisah tanpa shell atau output terminal, pipe nonblocking, deadline 2 detik dan terminasi grup proses pada kegagalan. Paste eksternal maksimum 1 MiB UTF-8, ditolak utuh; kosong/gagal tidak menghapus seleksi atau memakai data internal lama. Cut gagal mempertahankan sumber.
- Tambah konfigurasi TOML pengguna untuk theme/color/icons/clipboard dengan prioritas bawaan < file < CLI. `--config`, `--no-config`, `--save-config` dan `--clipboard` tersedia. Simpan preferensi eksplisit bersifat atomik/private dan keluar tanpa TUI; startup biasa tidak menulisnya. Nerd Font dapat dipilih sekali tanpa mengulang flag pada launch berikutnya.
- File konfigurasi maksimum 64 KiB, unknown key/type/value ditolak sebelum raw mode, tanpa project config/eksekusi. Help/version/probe independen; target save link/non-regular ditolak.
- Build tanpa warning, tes terarah dan 11 sesi PTY Linux dibuktikan, termasuk X11+Wayland nyata terisolasi, shared undo, dialog, helper timeout dan konfigurasi rusak. Detail/batas di [ROADMAP.md](docs/ROADMAP.md). Tidak menginstal helper atau mengubah konfigurasi/font sistem; session restore tetap backlog.

### Performa Markdown, TypeScript/TSX dan gap layout

- Backend highlight diganti dari regex-fancy ke Oniguruma native. Token viewport yang keluar layar dipindah tanpa clone span ke FIFO recent maksimal 512 baris/2 MiB; edit/path/language/barrier/close menginvalidasi cache terkait dan gap besar tidak ditokenisasi.
- Parser memakai budget baris plus slice kooperatif 4 ms; frame pertama dilukis sebelum cold highlight dan tidak ada beberapa slice berulang sebelum frame/input yang sama. Idle berhenti mengerjakan parser setelah pending selesai; satu regex tidak dipotong paksa.
- Grammar TypeScript/TSX asli berlisensi, termasuk type/generic/template dan JSX, dibundel dari konversi bat. Build menghasilkan dump embedded uncompressed; SyntaxSet/regex data dibagi immutable lewat LazyLock. Runtime tidak memuat YAML grammar/proyek/jaringan.
- `cargo run` kini dev opt-level 1 dengan debug info/assertions; profil test tetap 0. Dua gap vertikal header–body–dock dihapus, sehingga overhead normal 9→7 baris tanpa menghapus frame/helper atau mengubah minimum compact.
- Benchmark input tetap serta CPU scroll, rerun tes cache/grammar/chrome, build dan delapan sesi PTY dibuktikan; angka dan urutan pemeriksaan di [ROADMAP.md](docs/ROADMAP.md). Validasi mencakup shared edit/save/undo, long Markdown jump, TS/TSX, mouse hit dan restore terminal. Native build memerlukan compiler C; notice disertakan.

### Helper berkelompok, ikon solid dan browser folder

- Helper menjadi baris biasa `[tombol] Aksi · …` di bawah frame status; label tidak seluruhnya bold/berwarna. Grup lengkap tetap clickable, separator tidak menjadi aksi dan shortcut editing tidak diganti.
- Ikon Nerd memakai Material Design solid terverifikasi dan modifier bold; Unicode/ASCII tetap tersedia. Background header/explorer/tab/editor/gutter/current line/gap/status/dialog seragam di `canvas`; token surface/raised dihapus. Seleksi/fokus semantik tetap dibedakan.
- Explorer: Enter/helper Open masuk folder, Backspace/helper Up/klik `..` naik; Left/Right dan klik folder tetap tree expand/collapse. Header/root sinkron setelah listing berhasil, tanpa menutup dokumen atau mengubah CWD. Root `/` no-op; listing gagal mempertahankan browser; directory-symlink swap ditolak dengan NOFOLLOW.
- Tambah tujuh contoh di `testfile`: YAML/YML/JS/HTML/CSS ter-highlight; TS/TSX masih plaintext karena bundled grammar belum tersedia. Tidak ada frontend/dependency baru yang dijalankan.
- Format/build, jalur unit lintas modul dan rerun 33 tes Explorer lolos; PTY membuktikan helper/mouse/parent/empty/root/refresh/permission recovery/race/shared save/undo/minimum/restore pada empat profil. Retry menyinkronkan root/header canonical setelah pulih. Detail urutan unit di ROADMAP.md. Font/config pengguna tidak diubah; history Back/Forward belum tersedia.

### Redesign visual bergaya superfile

- Explorer dan setiap editor/split kini memakai frame penuh membulat, judul/ikon terintegrasi dan penanda fokus accent/bold plus marker non-color. Tab menampilkan ikon kategori file; Unicode default memakai glyph grafis, Nerd opt-in dan ASCII tetap tersedia.
- Header workspace serta dock status/helper berbingkai saat ruang cukup. Gap satu sel memisahkan kartu dan tetap dapat di-drag; gap global memakai canvas tema eksplisit. Chrome compact dipilih sebelum mengorbankan panel yang masih muat.
- Geometry/minimum dipusatkan di `chrome` untuk renderer/controller/workspace. Minimum panel 12×4, editor tunggal compact 12×6; preflight split menghitung ruang compact maksimum.
- Border hanya memfokuskan panel; navigasi gutter dipertahankan. Wheel bekerja di seluruh kartu tanpa mencuri fokus. Paging, tab close dan drag memakai inner/outer geometry baru.
- Format, 217 unit test, build dan PTY nested frame/mouse/paging/tab/compact/collapse/minimum lolos. Empat profil tema/warna/ikon diperiksa; raster Nerd Mono dari grid PTY tersedia untuk inspeksi, bukan klaim validasi emulator/font fisik.

### Pemolesan field, bantuan dan chrome

- Marker fokus panel/explorer dipadatkan; footer memprioritaskan aksi inti. Status memakai metadata path/format/dirty terpisah, dengan prioritas error dan buffer yang dipakai ulang.
- Field path/Find/Replace mendukung caret/seleksi grapheme, navigasi tengah teks, Delete, clipboard internal dan klik/drag. Batas 4096 byte memperhitungkan normalisasi/replacement; penolakan tidak membuang isi/seleksi.
- Bantuan F1 berbagi katalog dengan `--help`, mendukung scroll/page/wheel/resize, dan mengembalikan modal/field serta pending Save/Close/Quit. Tombol yang terlihat memakai hit geometry dan aksi statis, bukan parsing nama file.
- Regresi ditutup untuk caret sesudah penggabungan grapheme, render Unicode tiga sel tanpa byte/control korup, gap kata help dan scroll lama sesudah resize.
- Format, 204 unit test, build dan alur PTY field/mouse/help/shared-search/save/quit lolos. Empat profil tema/warna/ikon diperiksa; font fisik/Nerd Font dan desain visual final belum diklaim.

### Hybrid panel + tab dan shared document

- F8/F9 split dua orientasi rekursif, F10/Shift+F10 pindah panel, F12 close panel. Tab tetap lokal per panel; Alt+Shift+Arrow dan drag mouse me-resize divider sesuai minimum subtree.
- Registry memiliki satu Document/history/path/baseline per identitas file. View memiliki cursor/selection/scroll independen, termasuk ketika hidden. Edit, undo/redo dan Replace All memetakan ulang posisi seluruh view melalui TextChange.
- Open/explorer menargetkan panel terakhir; alias lintas panel membuat view shared, bukan buffer terpisah. Save/Save As/dirty/read-only berlaku bersama; Save As mengecualikan dokumen sendiri dari collision. Reload memperbarui semua view.
- Close satu shared view tidak meminta discard; guard berlaku pada final view. Close panel memeriksa seluruh dokumen final, quit dirty sekali per dokumen. Save gagal/cancel menghentikan continuation tanpa kehilangan shared survivor.
- Layar kecil menyembunyikan explorer/panel sementara tanpa menghapus tree/ratio; split baru ditolak bila minimum tidak muat. Mouse tab/body/close, wheel tanpa focus steal dan drag asal-pane memakai hit ID stabil; scroll/resize menginvalidasi frame stale.
- Highlight cache per dokumen menggabungkan hanya viewport terlihat, bukan gap besar antarpanel. Budget/checkpoint/long-line policy tetap berlaku. UI memakai reusable geometry dan tab hit buffers.
- 178 unit test lolos dan build tanpa warning. PTY membuktikan shared edits/history/save/rename, nested resize/collapse, hidden focus, mouse/drag, syntax union, close-pane multi-doc dan quit dedup. Desain visual tetap terbuka untuk pemolesan tahap berikutnya.

### Workspace satu panel

- Argumen folder/default cwd membuka explorer lazy, folder-first, termasuk dotfiles. Keyboard/mouse expand/open/refresh, wheel per area dan divider 18–40 kolom; narrow screen menyembunyikan explorer sementara.
- Beberapa tab dengan ID stabil, history/view sendiri, dedup dev/inode/canonical alias, marker dirty/read-only dan kontrol overflow/close. Ctrl+N/O menambah tab; Alt+Left/Right atau Ctrl+PageUp/Down berpindah; Ctrl+W close dan Ctrl+Q memeriksa semua dirty.
- Save As ke target tab lain ditolak. Dialog bertarget ID; dirty tab nonaktif ditampilkan sebelum close; Save As cancel/failure menghentikan close/quit. Discard per dokumen tidak memalsukan checkpoint saved.
- Ctrl+F Find, **Ctrl+R Replace** (Ctrl+H tetap Backspace), F3/Shift+F3 wrap, F6 replace-current tervalidasi dan F7 Replace All satu undo. Literal case-sensitive, grapheme-safe, Unicode/multiline; field 4096 byte, jendela hasil komposisi 64 MiB.
- Syntect menjadi dependency runtime, semantic highlighting dengan checkpoint 128 baris, budget 200 baris/update, token viewport dan invalidasi generation termasuk undo/redo. Batas 16 KiB/baris/error parser menghasilkan warning dan plaintext, bukan token stale.
- Tema Dark/Light/Terminal, auto/truecolor/256/16/none serta Unicode/Nerd/ASCII tersedia melalui CLI. Palette RGB/xterm-256 diuji kontras; mono mempertahankan fokus/seleksi non-color. Nerd Codicons opt-in, font tidak dipasang/dideteksi.
- Perbaikan ellipsis UTF-8 satu kolom, traversal child/ancestor symlink, scroll explorer pasca-redraw dan klik terhadap frame stale. Geometry layout dipertahankan terpisah dari validitas hit.
- Batch workspace satu panel sebelumnya lolos 140 unit test dan PTY tab/explorer/resize, search/replace/undo, dirty/save, syntax dan matriks tema; cakupan hybrid terbaru tercatat di atas dan [ROADMAP.md](docs/ROADMAP.md).

### Editor satu dokumen

- `notefart` / `cargo run` kini membuka editor kosong; argumen file membuka dokumen atau menyiapkan path baru tanpa membuat file sebelum Save. `--help`, `--version`, `-- FILE` dan `--probe` tetap tersedia.
- Document berbasis Ropey, delta undo/redo dan saved checkpoint dengan state ID monotonik. Navigasi/delete, word selection, cursor dan hit testing menghormati grapheme, chunk seam, combining mark, CRLF dan sel terminal.
- Editing langsung, seleksi keyboard/mouse, klik/double-click/drag/autoscroll, navigasi dan clipboard **internal**. Ctrl+C tidak keluar dari editor; Ctrl+Q/W memakai Save/Discard/Cancel. Paste valid satu transaksi undo.
- New/Open dialog, Ctrl+S Save, F4 Save As dan konfirmasi target existing termasuk path dokumen saat ini. Save gagal/cancel tidak menyelesaikan permintaan keluar atau membersihkan dirty state.
- UTF-8/BOM/LF/CRLF round-trip; mixed/lone CR read-only tanpa normalisasi implisit. Invalid UTF-8, NUL dan non-regular file ditolak. Batas load/save 64 MiB.
- Fingerprint isi + identitas/metadata mendeteksi konflik eksternal, dengan overwrite eksplisit, Save As, Reload/discard lokal, dan Cancel.
- Tempfile satu direktori, atomic replacement/no-clobber, file dan directory sync; mode/uid/gid/xattrs/ACL dipertahankan atau gagal eksplisit. Metadata keamanan diperiksa ulang meski overwrite diizinkan. File baru tetap 0600 tanpa mengubah umask proses; save langsung ke symlink/hard link ditolak.
- UI Dark dengan header, nomor baris, seleksi, cursor, status dan shortcut helper. Gutter yang berubah ikut menjaga cursor terlihat; dialog kurang dari 20×6 menawarkan resize/Esc dan menolak keputusan tersembunyi.

### Cutover input terminal

- Editor dan probe memakai decoder streaming berbatas pada thread utama; thread pembaca dan decoder event Crossterm lama dihapus.
- Batas paste 1 MiB; overflow/invalid UTF-8 dibuang utuh sampai terminator, tanpa menjalankan kontrol di payload. CSI/UTF-8/paste fragment tetap aman melintasi polling kosong; parser meresinkronisasi ESC baru, mengabaikan OSC/string-control sampai terminator dan mendukung SGR mouse.
- `--probe` tetap alat diagnostik input, resize dan Unicode. Ctrl+C hanya keluar dalam mode probe; editor memakainya untuk copy internal.
- Contoh `text_probe` dan `highlight_probe` tetap alat evaluasi terpisah; editor memakai engine produksi sendiri.

### Verifikasi dan batas

- Batch editor satu dokumen sebelumnya lolos 70 test; workspace satu panel 140; hybrid shared-view 178; polish field/help 204; redesign visual terbaru 217. Bukti tiap tahap dan batas lingkungan ada di [ROADMAP.md](docs/ROADMAP.md).
- Probe dengan decoder baru diuji melalui tmux 3.5a terisolasi; fixture dan server pengujian sudah dibersihkan. Detail ada di [ROADMAP.md](docs/ROADMAP.md).
- Belum tersedia: clipboard sistem/OSC 52, autosave/crash recovery dan konfigurasi persisten. UI/UX masih tahap pemolesan; glyph/font fisik/Nerd Font belum dibuktikan.
- Batas lain: filesystem sinkron lambat, final check→rename race, matriks ACL/security label, panic/syscall fault injection, suspend eksternal, SSH, OS/font/protokol lain. Tidak ada klaim keamanan/performa universal; lihat [SECURITY.md](docs/SECURITY.md).
