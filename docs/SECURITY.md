# Keamanan dan Integritas Data Notefart

Status: kontrol editor hybrid panel + tab/shared document diverifikasi pada cakupan Linux/PTY yang dinyatakan. Ini **bukan jaminan keamanan seluruh filesystem/platform**. Pemolesan UI/UX dan matriks platform lebih luas masih terbuka; mekanisme pelaporan kerentanan khusus belum ditetapkan.

## Model ancaman

Aset utama: isi dokumen, file asli, clipboard, dan kondisi terminal pengguna.

Input tidak tepercaya meliputi isi file, nama/path file, isi clipboard, struktur direktori, serta metadata konfigurasi/grammar pihak ketiga. Proyek yang dibuka tidak otomatis dipercaya.

Ancaman utama:

| Ancaman | Kontrol yang direncanakan |
| --- | --- |
| Kehilangan perubahan | Dirty checkpoint, dialog penutupan, buffer tetap hidup setelah save gagal |
| Menimpa edit eksternal | Pemeriksaan snapshot disk dan dialog konflik sebelum overwrite |
| Escape sequence berbahaya | Render kontrol sebagai representasi aman; jangan meneruskan byte file/nama file ke terminal |
| Eksekusi dari proyek | Tidak menjalankan hook, command, plugin, atau konfigurasi executable secara otomatis |
| Kebocoran melalui clipboard | Copy eksplisit, backend terlihat, OSC 52 opt-in pada rancangan awal |
| Terminal tertinggal raw mode | Lifecycle guard dan restorasi pada exit/error yang dapat ditangani |
| Input berukuran ekstrem | Ukur dan batasi pekerjaan/memori; error yang jelas, bukan truncation diam-diam |

## Membuka file dan direktori

- Jangan mengubah file hanya karena dibuka.
- Validasi UTF-8 dan format sebelum mengizinkan editing/save. Format unsupported mengikuti [PRODUCT.md](PRODUCT.md).
- Deteksi regular file sebelum membaca; FIFO, socket, dan device tidak diperlakukan sebagai dokumen teks biasa.
- Traversal explorer lazy, termasuk dotfiles, maksimum 10.000 entri/direktori. Permission/cap error ditampilkan, tidak memotong hasil diam-diam dan tidak mengganti tree usable dengan hasil parsial.
- Setiap komponen anak dibuka dari descriptor root memakai `openat`/`NOFOLLOW`; metadata memakai `statat` tanpa follow. Symlink direktori, termasuk penggantian entri/ancestor setelah listing, tidak diekspansi. Root eksplisit boleh menunjuk direktori melalui symlink; root UI bukan sandbox keamanan.
- Navigasi parent/Enter hanya terjadi atas input pengguna. Root awal canonical dan boleh keluar dari direktori awal melalui parent; ini bukan pembatas akses filesystem. Enter pada directory tree membuka setiap komponen dengan `NOFOLLOW`, sehingga directory yang diganti symlink setelah listing ditolak. Listing baru disiapkan sebelum commit, termasuk batas 10.000 entri; kegagalan tidak membuang root/tree lama atau dokumen dirty dan tidak mengubah CWD proses.
- Dedup tab memakai identitas dev/inode dan canonical path, termasuk alias missing path melalui ancestor existing. `..` tidak diruntuhkan secara leksikal melewati symlink. Save As ke target milik dokumen lain ditolak, bahkan jika overwrite diminta; view lain dari dokumen yang sama bukan collision.

## Menyimpan file

Kebijakan save diterapkan per dokumen/tab:

1. Simpan metadata baseline dan fingerprint isi yang dimuat. Bandingkan target saat save, bukan hanya timestamp.
2. Perubahan, penghapusan, atau penggantian target memunculkan konflik. Pilihan: reload dengan perlindungan perubahan lokal, Save As, overwrite eksplisit, atau cancel. Tidak ada auto-merge awal.
3. Save As tidak menimpa file yang sudah ada tanpa konfirmasi.
4. Untuk regular file yang didukung, tulis snapshot ke temporary file di direktori target dengan izin awal terbatas, flush/sync sesuai platform, lalu replace dan sinkronkan direktori jika didukung.
5. Replacement mempertahankan mode, uid/gid dan xattrs/ACL melalui file descriptor atau menolak jika operasi metadata gagal. Sesudah copy, identity/mode/owner/ctime dicek ulang sebelum persist, termasuk pada overwrite eksplisit. Mode/uid/gid dan user xattr telah dibuktikan melalui runtime; seluruh varian ACL/security label belum divalidasi pada filesystem nyata.
6. Bersihkan temporary file pada kegagalan yang dapat ditangani. Jangan menghapus file asli untuk mencoba memperbaiki kegagalan replace.
7. Tandai revision yang benar-benar tersimpan sebagai checkpoint hanya setelah seluruh langkah save yang diwajibkan berhasil.

Atomic replace dapat mengganti inode dan memutus hubungan hard link. Save langsung ke final-component symlink dan file dengan beberapa hard link **ditolak**, termasuk pada overwrite eksplisit; Save As ke path regular baru tetap tersedia. Membuka untuk membaca dan mengedit buffer tetap diperbolehkan. Write-through link belum didukung.

Atomic replace bukan solusi universal: filesystem, mount jaringan, durability, dan perlombaan proses lain berbeda. Pemeriksaan konflik sebelum replace masih menyisakan race jika proses lain menulis setelah pemeriksaan. Notefart tidak boleh menjanjikan perlindungan concurrent writer yang absolut tanpa mekanisme tambahan yang terbukti.

Jika replace berhasil tetapi sync berikutnya gagal, laporkan ketidakpastian durability dan pertahankan status belum tersimpan; jangan mengklaim file di disk tidak berubah.

File baru tetap mode 0600 tanpa mengganti umask proses. Temporary file dibersihkan pada kegagalan sebelum persist. File di-sync setelah payload serta metadata selesai ditulis; direktori di-sync setelah persist. Kebijakan file maksimum 64 MiB dibatasi saat read dan encode, bukan hanya melalui stat awal.

UTF-8/BOM dan LF/CRLF dipertahankan. Mixed newline/lone CR dibuka read-only tanpa normalisasi diam-diam; NUL, invalid UTF-8 dan non-regular file ditolak. Open memakai descriptor nonblocking lalu fstat agar FIFO tidak menunggu writer.

### Integritas workspace, pencarian dan highlighting

- ID dokumen/view/panel stabil dan tidak dipakai ulang. Registry memiliki teks/history/baseline sekali; modal bertarget ID, bukan indeks. Dirty guard close hanya berlaku saat dokumen kehilangan view terakhir. Quit memeriksa setiap dokumen dirty sekali, termasuk yang memiliki banyak view.
- Close panel memeriksa seluruh dokumen final di dalamnya. Save gagal/Save As cancel menghentikan continuation; Discard hanya dokumen yang dipilih. Dokumen dengan view di panel lain dipertahankan. Panel terakhir mendapat blank pada close biasa; quit tidak memalsukan saved checkpoint untuk melewati guard.
- Setiap transaksi edit/undo/redo memetakan cursor, anchor dan viewport seluruh view, termasuk tab tersembunyi, melalui splice scalar dan boundary grapheme. Save/path/dirty/read-only shared; reload mereset seluruh view dokumen. Replace-current memvalidasi pane/view/document ID, generation, query dan seleksi. Read-only menolak F6/F7; Replace All satu undo dengan jendela komposisi maksimum 64 MiB, bukan batas total RSS/history.
- Field path/Find/Replace maksimum 4096 byte UTF-8 setelah normalisasi/replacement seleksi; input oversized ditolak atomik. Path menolak karakter kontrol sebelum mutasi. Paste Find/Replace menormalisasi CRLF/CR menjadi LF dan tidak menjalankan kontrol sebagai command. Pencarian literal/case-sensitive, bukan regex yang berasal dari proyek.
- Syntect/Oniguruma memakai budget 200 baris, slice kooperatif 4 ms, checkpoint 128 baris dan batas 16 KiB/baris. Token render hanya union viewport; FIFO recent dibatasi 512 baris/2 MiB payload per dokumen. Edit dari baris berubah, path/language/reload, barrier dan close menginvalidasi cache yang relevan; gap besar tidak dicache. Error menghasilkan warning/plaintext, bukan token stale. Slice tidak memotong satu operasi regex; ini bukan hard deadline CPU.
- Grammar TypeScript/TSX tepercaya dan berlisensi di-link ke grammar dasar saat build; runtime hanya mendeserialisasi dump embedded sekali. Tidak membaca grammar/config executable dari proyek, mengambil grammar melalui jaringan, atau menjalankan TypeScript/JS yang dibuka. Backend menambah library C Oniguruma, sehingga batas validasi native/platform dan kewajiban notice distribusi tetap dinyatakan.
- Snapshot geometry terpisah dari validitas hit. Edit, scroll keyboard/wheel/autoscroll, perubahan tree/tab, resize, serta perubahan teks/caret/seleksi/fokus field menolak koordinat stale sampai frame baru. Ukuran terminal disinkronkan setelah poll sebelum preflight split maupun keputusan modal/mouse.
- Hit field memetakan sel display ke byte sumber asli; escape dan seluruh sel grapheme lebar tidak menjadi offset UTF-8 parsial. Clipboard field hanya membaca/mengubah seleksi field yang fokus; Ctrl+Z/Y tidak mengubah dokumen belakang.
- Bantuan F1 menangguhkan dan mengembalikan modal beserta continuation Save/Close/Quit. Kembali dari bantuan bukan Cancel pada operasi semula; shortcut editor diblokir selama bantuan terbuka.
- Tombol dialog/helper memakai KeyEvent statis dan geometry yang benar-benar terlihat. Nama file, isi field dan status tidak diparse menjadi aksi, termasuk jika mengandung label seperti `[Q]`. Konfirmasi tetap melewati guard controller yang sama dengan keyboard.
- Frame/ikon tidak memperluas otoritas input: border hanya memfokuskan, gutter memakai mapping baris, dan wheel kartu/baris parent tidak memindahkan fokus. Parent row terpisah dari hit/scroll/page tree dan tidak menjadi Backspace pada dokumen. Helper hanya menerbitkan hit untuk kelompok aksi lengkap yang terlihat; separator tidak aktif. Geometry outer/inner serta minimum berasal dari `chrome`; collapse tidak membuang view/dokumen.


## Terminal dan input

- Isi file, nama file, path, dan pesan error harus melalui rendering aman. Escape/control character tidak boleh menjadi instruksi terminal, termasuk OSC dan hyperlink injection.
- Tab/newline diproses sebagai layout; karakter kontrol lain diberi representasi visual yang tidak ambigu.
- Bidi dan karakter tak terlihat perlu penanda yang bisa diaktifkan; penanda tersebut bukan perubahan isi file.
- Aktifkan raw mode, alternate screen, mouse capture, dan bracketed paste hanya melalui lifecycle terpusat; restorasi juga terpusat.
- Paste tidak ditafsirkan sebagai command atau shortcut. Isi yang berbahaya untuk rendering tetap disimpan sebagai data, bukan dieksekusi.
- Panic, error, dan signal yang bisa ditangani harus memicu restorasi terminal. SIGKILL dan mati listrik tidak bisa dijamin dipulihkan aplikasi.
- Suspend/resume harus dirancang eksplisit atau dinyatakan belum didukung; jangan meninggalkan terminal dalam state rusak.

### Input berbatas yang dipakai editor dan probe

- Keduanya menolak stdin/stdout non-TTY sebelum mengubah terminal. Raw mode, alternate screen, cursor, mouse, dan bracketed paste tetap dimiliki guard terpusat.
- Jalur `crossterm::event::read` dan thread pembaca lama sudah dihapus. Decoder streaming pada thread utama memakai readiness poll Rustix dan read maksimal 4096 byte; tidak menunggu sisa paste/escape di dalam read loop.
- Payload paste maksimum 1 MiB; overflow membuang seluruh payload lalu drain sampai terminator, hanya mempertahankan prefix delimiter kecil. Invalid UTF-8 paste juga ditolak utuh. CSI dibatasi 64 byte; buffer ingress punya retensi kecil berbatas terpisah dari payload. Tidak ada antrean event atau thread yang ditinggalkan saat shutdown.
- Timeout kosong tidak membuang prefix CSI/paste. Lone Escape memakai idle delay; ESC baru meresinkronisasi CSI lama. OSC/DCS/string-control yang tidak didukung dibuang sampai BEL/ST tanpa menerjemahkan body menjadi shortcut.
- Batas protokol: hanya bytes di dalam bracketed paste yang dikenali sebagai paste. Terminal yang mengirim paste sebagai ketikan biasa dapat memicu shortcut; sequence terminator pada wire memang mengakhiri paste. Kitty/Windows keyboard dan X10 mouse tidak diklaim didukung.
- Tampilan dokumen/path/status menetralkan kontrol terminal/Bidi. Dokumen dan field membuat grapheme zero-width standalone terlihat. Tab, combining mark, ZWJ dan karakter lebar memakai mapping cell yang konsisten untuk rendering, cursor dan hit testing; font/glyph fisik tetap bergantung terminal.
- Cache geometri hanyalah state turunan: checkpoint harus berada di batas grapheme, menyimpan kolom terminal sebenarnya, dan memakai konteks slice utuh saat resume. Edit/undo/redo menginvalidasi suffix sebelum Rope berubah; cache baris sesudahnya dibuang, clone memulai cache baru. Renderer tetap melakukan clipping, dan escape kontrol/Bidi tetap melalui jalur yang sama; cache tidak mengeksekusi atau meneruskan bytes kontrol mentah.
- Ctrl+C/X/V memakai backend clipboard terpilih hanya pada editor/field yang fokus. Ctrl+W menutup tab; Ctrl+Q memeriksa semua dirty. Dialog umum di bawah 20×6 dan Find/Replace di bawah 40×10 menolak input/konfirmasi tersembunyi, tetapi Esc tetap membatalkan.
- Bit mode hanya dibersihkan setelah pemulihan berhasil; Interrupted diulang dan Drop dapat mencoba lagi. Panic hook sudah direview statis, tetapi belum diuji melalui fault injection panic/syscall.
- Cleanup SIGTERM saat paste parsial terbukti dengan decoder baru pada editor dan probe. Hasil batch awal untuk SIGHUP/SIGINT ada di roadmap; tidak ada janji cleanup tepat waktu ketika filesystem sinkron sedang macet.
- Tidak ada autosave/crash recovery. Signal terminasi melaporkan edit belum disimpan tanpa menulisnya. Suspend/resume eksternal belum didukung; SIGKILL/mati listrik/terminal hilang tidak menjamin restorasi.
- Log probe hanya menampilkan preview dalam memori; jangan paste rahasia saat layar direkam. Editor tidak mencatat isi dokumen/clipboard ke disk. Snapshot buffer/session belum tersedia; konfigurasi hanya memuat preferensi.


## Clipboard dan privasi

- Tidak membaca clipboard sistem secara berkala; baca hanya saat aksi paste eksplisit.
- Tidak menulis clipboard eksternal hanya karena pengguna menyeleksi teks.
- Clipboard internal tidak boleh dilaporkan sebagai keberhasilan clipboard sistem.
- Backend lokal memakai executable tetap `wl-copy`/`wl-paste` atau `xclip` dengan argv terpisah, tanpa shell dan tanpa stderr/stdout helper ke terminal. Tidak ada command clipboard dari konfigurasi proyek.
- Auto pada sesi SSH memakai internal untuk privasi; akses display forwarding memerlukan pilihan `system` eksplisit. Ketiadaan backend dan kegagalan eksternal terlihat sebagai status, bukan klaim keberhasilan palsu.
- Cut menghapus seleksi hanya setelah backend terpilih menerima copy. Gagal write mempertahankan sumber; gagal read tidak menyisipkan teks internal yang lama. Input eksternal UTF-8 dibatasi 1 MiB dan ditolak utuh saat tidak valid/oversized; hasil kosong tidak menghapus seleksi.
- I/O helper mempunyai deadline 2 detik dan reap grace 250 ms setelah terminasi grup proses milik helper; tidak ada pipe/join yang ditunggu tanpa batas. Ini operasi saat aksi pengguna, bukan pekerjaan render/idle; helper owner yang berhasil boleh mempertahankan clipboard sesuai protokol desktop. Batas tersebut bukan jaminan scheduling/syscall OS real-time.
- OSC 52 belum diimplementasikan. Jika kelak ditambahkan, write memerlukan opt-in dan aksi copy; tidak ada OSC clipboard read otomatis. Ketersediaan serta konfirmasi terminal berbeda-beda.
- Tanpa telemetry atau pengiriman isi file pada rancangan awal.
- Log tidak memuat isi dokumen/clipboard. Path juga berpotensi sensitif; batasi log dan jelaskan lokasi log jika logging ditambahkan.
- Clipboard internal dan buffer berada di memori proses; belum ada janji secure erase, perlindungan swap, atau perlindungan dari proses lain dengan hak setara.

## Dependency dan konfigurasi

- Pilih dependency terawat dan minimum; review lisensi, advisory, serta kebutuhan native code saat pin versi.
- Grammar/theme tidak boleh menjalankan kode proyek secara implisit.
- Helper clipboard menggunakan path executable dan argv terpisah; tidak ada shell command yang dibangun dari teks clipboard, nama file atau konfigurasi.
- Hanya konfigurasi preferensi pengguna yang dibaca, bukan konfigurasi proyek: empat key string `theme`, `color`, `icons`, `clipboard`, maksimum 64 KiB UTF-8. Tipe/key/nilai yang tidak dikenali gagal sebelum raw mode; help/version/probe tidak bergantung pada isi konfigurasi.
- Lokasi default mengikuti XDG_CONFIG_HOME absolut atau HOME absolut; tidak memakai CWD sebagai fallback. Flag CLI menang atas konfigurasi; `--no-config` membypassnya. Startup tidak menulis file. Penyimpanan eksplisit `--save-config` memakai replacement atomik dan menolak target link/non-regular, bukan overwrite langsung.
- Preferensi bukan snapshot sesi atau autosave; tidak menyimpan isi buffer, path tab, clipboard, token atau rahasia proyek. Konfigurasi proyek tidak dapat menimpa kebijakan keamanan pengguna.

## CI dan artifact build

- Workflow memakai event `push`/`pull_request`/manual pada runner GitHub-hosted Ubuntu, bukan `pull_request_target` atau self-hosted runner. Job hanya mendapat `contents: read`; checkout tidak mempertahankan credential dan tidak ada PAT/deploy secret atau izin publish yang ditambahkan.
- Action dipin full commit SHA; toolchain Rust dipin dan Cargo memakai `--locked` untuk tes/build. Pin mengurangi perubahan tak sengaja, bukan pengganti audit dependency/Action atau jaminan build bit-identical.
- Artifact hanya memuat binary, template konfigurasi dan notice yang disebut eksplisit, tidak mengunggah seluruh workspace/config/log. Masa retensi 7 hari; siapa yang dapat mengunduh mengikuti visibilitas/izin repository. Workflow tidak membuat repository menjadi private.
- `.gitignore` bukan secret scanner dan tidak menghapus credential yang sudah tracked. Review source sebelum push pertama. Audit lisensi/dependency menyeluruh dan kesiapan distribusi publik tetap terbuka; notice yang ikut artifact bukan klaim seluruh kewajiban distribusi sudah selesai.
- Build/tes/packaging dibuktikan lokal; akses GitHub, eksekusi runner Ubuntu dan upload/download artifact jarak jauh belum dibuktikan sampai repository tujuan tersedia.

## Bukti minimum sebelum rilis

Uji file dengan escape sequence dan nama berkarakter kontrol; save gagal dan disk penuh melalui fault injection yang aman; perubahan file eksternal; Save As ke target existing; symlink/hard link; file non-regular; serta exit/error dan restorasi terminal. Jalankan dengan direktori sementara dan fixture terisolasi, bukan file pengguna.
