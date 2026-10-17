# Cuckoo 2.0.0

Windows向けの時報アプリです。製作者：ひろやまひろし / Hiroyama Hiroshi。

[インストール・利用ガイド](docs/USER-GUIDE-ja.md)

## 現在の機能

- 48枠の時報設定、00分・30分の音色と1回／2回の個別設定。
- 内蔵音色4種類：鳩時計・教会ベル・振り子時計・学校のチャイム。学校の2回設定は長い版1回。
- カスタムMP3の登録・改名・試聴・解除。プリセットは48枠のみを保存。
- Rustによる時刻・音声・トレイ管理。格納時はWebViewを破棄し、復帰時に再生成。
- v8設定の専用領域への保存・バックアップ。従来WAVの素材と再生経路は削除済み。旧選択はバックアップ後に鳩時計MP3へ移行。
- ライト／ダーク、最小表示、共通カスタムセレクトとローカルSVG。

正式版の識別子は jp.hiroyamahiroshi.cuckoo です。正式版専用の新規設定で開始し、開発版・Electron版の設定を自動読込・移行しません。正式版の更新時は既存の正式版設定を維持します。

## 開発・確認

Windows x64、Node.js/npm、Rust MSVC、Visual Studio C++ Build Tools、Windows SDK、WebView2が必要です。

```powershell
npm.cmd ci
npm.cmd run build
npm.cmd test
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
npm.cmd run tauri -- build
```

通常ビルドのEXEは src-tauri/target/release/Cuckoo.exe、NSISインストーラーは src-tauri/target/release/bundle/nsis/ に生成されます。
開発起動は npm.cmd run tauri:dev を使用し、正式版と設定領域を分離します。配布ビルドでは診断用featureを有効にしません。
テストは合成データと test-runtime/ の隔離領域を使用します。実利用設定をテストに使用しないでください。

## 配布・ライセンス

NSISのユーザー単位インストーラーを使用します。WebView2未導入時はインターネット接続が必要です。
コード署名はありません。SmartScreen等の警告が表示される場合があります。正規配布元とSHA-256を確認してください。
素材・依存ライブラリの通知は licenses/ にあり、インストール先にも同梱されます。

## 既知の制約

保存成否不明時の復旧・終了UX、戻らないCOM/WebView生成の安全な中断、長期メモリ収束には既知の残件があります。
