# AGENTS.md — Notefart

Aturan kerja untuk agen yang berkontribusi pada repository ini. Berlaku dari root ke seluruh direktori, kecuali aturan yang lebih spesifik menetapkan batas tambahan. File ini tidak menjalankan worker secara otomatis dan tidak menggantikan instruksi sistem, developer, atau permintaan pengguna yang berlaku.

## 1. Bahasa dan arah produk

- Gunakan bahasa Indonesia untuk diskusi, rencana, dan dokumentasi; ikuti konvensi kode yang sudah ada untuk identifier dan komentar.
- Notefart adalah editor teks terminal dengan Rust dan Ratatui sebagai arah teknologi, kontrol familiar tanpa mode editing wajib, serta keyboard dan mouse yang sama-sama didukung.
- Pertahankan explorer, syntax highlighting, tab per panel, split panel, dan shortcut helper sebagai arah produk. Produk akhir tetap editor teks familiar ala Notepad, bukan editor modal atau IDE; jangan mengarahkan roadmap menuju IDE.
- Jangan menganggap isi mockup sebagai fitur yang sudah diimplementasikan atau kontrak dependency.

## 2. Acuan dan keputusan

Baca bagian yang relevan sebelum merancang atau mengubah perilaku:

| Acuan | Tanggung jawab |
| --- | --- |
| [docs/README.md](docs/README.md) | Arah yang disepakati dan indeks dokumentasi |
| [docs/PRODUCT.md](docs/PRODUCT.md) | Kebutuhan, cakupan, dan kriteria penerimaan |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Batas komponen, model state, dan invariant |
| [docs/DESIGN.md](docs/DESIGN.md) | Interaksi, tema, warna, font terminal, ikon, dan layout |
| [docs/SECURITY.md](docs/SECURITY.md) | Integritas file, terminal, clipboard, dan input tidak tepercaya |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Urutan implementasi dan bukti kelulusan |

- Pisahkan keputusan yang disepakati, usulan draft, dan perilaku yang benar-benar tersedia. Jangan menaikkan status draft menjadi keputusan final secara diam-diam.
- Delegator boleh memutuskan detail implementasi yang masih berada dalam cakupan pengguna; catat alasan dan selaraskan dokumen terkait sebelum mengirim kontrak ke worker.
- Keputusan yang mengubah cakupan, berisiko kehilangan data, atau menimbulkan tradeoff produk material harus dibawa kepada pengguna dengan rekomendasi yang konkret.
- Jika dokumen, kode, dan permintaan pengguna berbeda, identifikasi perbedaannya. Perilaku kode membuktikan kondisi saat ini, bukan otomatis membenarkan perubahan kontrak.
- Jangan menyalin spesifikasi teknis panjang ke file ini. Perbarui sumber acuan yang tepat.

## 3. Peran

### Delegator / integrator — agen utama

Agen utama memiliki tanggung jawab end-to-end:

1. Memahami kebutuhan, memeriksa kondisi repository, dan menentukan acceptance criteria.
2. Menentukan struktur, batas modul, model state, serta kontrak antarkomponen sebelum membagi pekerjaan.
3. Memetakan dependensi dan kepemilikan file; menyelesaikan prasyarat bersama secara inline.
4. Mengirim tugas mandiri yang lengkap kepada worker, lalu mengerjakan bagian integrasi atau pekerjaan independen yang tersisa.
5. Menyelesaikan konflik, memeriksa hasil worker, memperbarui caller dan dokumentasi, serta memverifikasi perilaku akhir.
6. Melaporkan hasil dan keterbatasan berdasarkan bukti, bukan sekadar status sukses worker.

Delegasi tidak memindahkan tanggung jawab desain, kualitas, atau kelengkapan dari agen utama.

### Worker implementasi

- Kerjakan target yang diberikan sesuai kontrak dan batas file.
- Baca pola kode serta dokumentasi yang relevan; jangan mengandalkan konteks percakapan yang tidak diberikan.
- Jangan memperluas scope, mengganti kontrak bersama, menambah dependency, mengubah struktur lintas modul, atau mendelegasikan lagi tanpa persetujuan delegator.
- Jika menemukan hambatan kontrak, laporkan fakta, lokasi, dampak, dan solusi yang disarankan. Lanjutkan pekerjaan independen yang aman; jangan menebak kontrak atau membuat fallback palsu.
- Jangan menyentuh file milik tugas lain. Minta delegator menangani kebutuhan perubahan lintas batas.
- Jangan menjalankan build, lint, test, atau formatter selama pekerjaan worker berlangsung. Sertakan usulan pemeriksaan untuk dijalankan terkoordinasi oleh integrator.
- Laporkan file dan simbol yang berubah, perilaku yang ditambahkan, asumsi, risiko, serta pemeriksaan yang belum dijalankan.

### Reviewer

- Tinjau secara read-only, kecuali secara eksplisit ditugaskan sebagai implementer untuk perbaikan tertentu.
- Fokus pada correctness, invariant, integritas data, keamanan, dan kesesuaian kebutuhan; jangan memperluas review ke refactor kosmetik yang tidak terkait.
- Temuan harus menyertakan lokasi, skenario pemicu, dampak, dan rekomendasi yang dapat ditindaklanjuti.
- Gunakan review terpisah untuk perubahan berisiko seperti save, shared document/undo, lifecycle terminal, dan input/clipboard; bukan kewajiban membuat reviewer untuk setiap edit kecil.
- Reviewer memberi temuan, bukan mengambil alih keputusan integrasi atau menyatakan tes lolos tanpa bukti.

## 4. Kapan dan bagaimana mendelegasikan

- Agen utama melakukan scoping awal sendiri sebelum membagi pekerjaan.
- Delegasikan implementasi ketika ada sedikitnya dua bagian independen yang cukup substansial, atau kebutuhan riset yang terlalu luas untuk dikerjakan inline tanpa membanjiri konteks. Jangan membuat tugas tambahan hanya agar terlihat paralel.
- Perubahan kecil, pertanyaan langsung, dokumentasi singkat, cleanup, dan satu bagian yang sudah jelas dikerjakan langsung oleh agen utama.
- Gunakan agen eksplorasi read-only untuk riset subsystem yang belum terpetakan; jangan membuka pekerjaan sederhana dengan delegasi riset.
- Kirim bagian independen dalam satu batch paralel. Urutkan hanya pekerjaan yang benar-benar memiliki dependensi.
- Setiap file punya satu pemilik penulisan pada satu waktu. File bersama dimiliki integrator atau satu worker yang ditunjuk; worker lain menerima kontraknya.
- Jika batas tugas berubah, perbarui pembagian kepemilikan sebelum edit berlanjut. Jangan menimpa pekerjaan pengguna atau worker lain.
- Tanpa worktree, branch, atau commit tambahan kecuali diminta atau memang diperlukan dan telah disepakati. Jangan menganggap worker memakai filesystem terisolasi.
- Jika tool subagent tidak tersedia, jelaskan keterbatasannya dan kerjakan langsung; jangan mengklaim delegasi yang tidak terjadi.

## 5. Kontrak minimum setiap tugas worker

Delegator harus memberikan informasi berikut, bukan hanya judul tugas:

| Bagian | Isi wajib |
| --- | --- |
| Tujuan | Perilaku pengguna yang harus berubah dan alasan perubahan |
| Konteks | Acuan dokumen, keputusan relevan, serta kondisi kode yang sudah diketahui |
| Kepemilikan | File/modul yang boleh diedit, file bersama, dan bagian di luar scope |
| Antarmuka | Tipe, API, event, state, error, atau format data yang disepakati; siapa pemiliknya |
| Dependensi | Prasyarat yang sudah tersedia dan hal yang harus menunggu |
| Acceptance | Skenario sukses, boundary, error, dan invariant yang harus dipenuhi |
| Verifikasi | Pemeriksaan yang disarankan; larangan menjalankan build/lint/test/formatter selama worker aktif |
| Laporan | Perubahan, asumsi, risiko, blocker, dan status verifikasi yang sebenarnya |

Gunakan path file dan nama simbol konkret ketika tersedia. Jika kode belum ada, delegator menetapkan kontrak awal terlebih dahulu; worker tidak merancang kontrak bersama secara terpisah.

## 6. Batas engineering

- Utamakan correctness, integritas data, dan maintainability; gunakan desain sederhana sebelum abstraksi generik.
- Pisahkan state editing, I/O, input, dan rendering sesuai arsitektur. Rendering tidak melakukan operasi file atau clipboard.
- Satu dokumen memiliki satu sumber teks dan riwayat undo; cursor, seleksi, serta viewport lokal per view.
- Bedakan byte UTF-8, posisi teks, grapheme, dan sel terminal. Hit testing harus konsisten dengan rendering.
- Hindari penyalinan seluruh dokumen atau alokasi per-frame yang bisa dihindari; optimasi lebih lanjut berdasarkan pengukuran.
- Jangan menghilangkan error, mengklaim clipboard eksternal sukses karena clipboard internal berhasil, atau membersihkan dirty state ketika save gagal.
- Jangan meneruskan escape sequence dari isi file/path ke terminal atau menjalankan kode proyek secara otomatis.
- Tema menggunakan token semantik. Font diatur melalui terminal; ikon memiliki fallback Unicode/ASCII tanpa menghilangkan fungsi.
- Dependency baru memerlukan keputusan delegator, alasan kebutuhan, serta pemeriksaan kompatibilitas dan lisensi. Toolchain dan versi tidak dikarang dari mockup.
- Migrasikan seluruh caller yang terdampak dan hapus jalur obsolete dari perubahan tersebut. Jangan menambahkan shim, stub, mock produksi, atau TODO sebagai pengganti implementasi yang diminta.
- Pertahankan perubahan pengguna yang tidak terkait. Jangan reset, clean, menghapus file, atau melakukan operasi destruktif tanpa otorisasi yang sesuai.

## 7. Integrasi, review, dan verifikasi

1. Setelah satu batch kerja selesai, periksa hasil terhadap kontrak dan integrasikan seluruh perubahan terkait, termasuk caller dan test yang rusak.
2. Lakukan review terpisah bila risiko memerlukannya. Delegator menangani temuan sebelum menganggap hasil selesai.
3. Setelah edit worker berhenti, integrator menjalankan pemeriksaan terkecil yang membuktikan perubahan, satu putaran terkoordinasi. Jika ada kegagalan, perbaiki lalu ulangi pemeriksaan yang terdampak; larangan pemeriksaan berulang bukan alasan meninggalkan kegagalan.
4. Gunakan command dari repository/CI yang benar-benar tersedia. Jangan menulis klaim bahwa `cargo test`, lint, atau aplikasi telah dijalankan sebelum command tersebut ada dan dieksekusi.
5. Perubahan TUI harus dibuktikan melalui aplikasi nyata: input, mouse, state, resize, serta output yang relevan. Snapshot mockup bukan bukti runtime.
6. Gunakan unit/integration test untuk invariant dan perilaku, bukan wiring, wording, salinan konfigurasi, atau source text. Tambahkan regression test bila jalur bug belum tercakup.
7. Uji I/O berisiko melalui fixture dan direktori sementara; jangan memakai file pengguna untuk simulasi kehilangan data atau disk penuh.
8. Untuk perubahan dokumen saja, periksa tautan, struktur, serta konsistensi kontrak; tidak perlu build aplikasi.
9. Jika runtime tidak tersedia, gunakan smoke check yang relevan bila memungkinkan dan nyatakan batas bukti. Jangan menyatakan kriteria runtime terpenuhi.

Tidak perlu menjalankan suite luas, audit seluruh repository, atau pemeriksaan berulang yang tidak relevan dengan perubahan.

## 8. Dokumentasi dan kriteria selesai

Pekerjaan selesai hanya jika:

- Perilaku end-to-end dan seluruh acceptance criteria yang diminta terpenuhi.
- Seluruh caller, test, dan dokumentasi terdampak sudah diperbarui atau sengaja tidak diubah dengan alasan yang jelas.
- Hasil worker telah diperiksa oleh integrator; temuan relevan ditangani.
- Pemeriksaan yang diklaim benar-benar dijalankan dan hasilnya diamati.
- Tidak ada scaffold, temporary script, atau fallback palsu yang tertinggal sebagai hasil akhir.
- Dokumen terkait diperbarui setelah perubahan permanen; changelog diperbarui ketika changelog implementasi sudah tersedia. Jangan membuat berkas tambahan yang tidak diperlukan.

Laporan akhir singkat: perubahan, bukti verifikasi, serta keterbatasan yang masih berlaku. Jangan menyembunyikan blocker, menandai bagian belum diuji sebagai selesai, atau mengurangi scope tanpa persetujuan pengguna.
