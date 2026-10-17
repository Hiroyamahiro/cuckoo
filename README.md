# Cuckoo 2.0.1

Windows向けのシンプルな時報アプリです。製作者：ひろやまひろし / Hiroyama Hiroshi

[インストール・利用ガイド](docs/USER-GUIDE-ja.md)

## 現在の機能

- 独自UIのインストーラー。更新時は設定・プリセット・追加音源を保持。
- 起動時・約24時間ごとの安定版更新確認。メニューから取得・SHA-256照合し、独自インストーラーで承認後に更新・再起動。
- 48枠の時報設定、00分・30分の音色と1回／2回の個別設定。
- 内蔵音色4種類：鳩時計・教会ベル・振り子時計・学校のチャイム。
- カスタムMP3の登録・改名・試聴・解除。プリセットは48枠のみを保存。
- Rustによる時刻・音声・トレイ管理。格納時はWebViewを破棄し、復帰時に再生成。
- v8設定の専用領域への保存・バックアップ。従来WAVの素材と再生経路は削除済み。旧選択はバックアップ後に鳩時計MP3へ移行。
- ライト／ダーク、ミニマム表示、共通カスタムセレクトとローカルSVG。
- 00分・30分を個別に操作できる「一括設定」、簡素化したタイトルバーとトレイ格納メニュー。
- 英数字フォントUrbanistをローカル同梱。日本語にはWindows標準フォントを使用し、フォント取得のための通信は行いません。

正式版の識別子は jp.hiroyamahiroshi.cuckoo です。正式版専用の新規設定で開始し、開発版・Electron版の設定を自動読込・移行しません。正式版の更新時は既存の正式版設定を維持します。

## 開発・確認

Windows x64、Node.js/npm、Rust MSVC、Visual Studio C++ Build Tools、Windows SDK、WebView2が必要です。

```powershell
npm.cmd ci
npm.cmd run build
npm.cmd test
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
npm.cmd run test:installer
npm.cmd run build:installer
```

配布用の独自GUIインストーラーは setup/dist/ に生成されます。ビルドには.NET Framework 4.8のコンパイラーも必要です。内部エンジン（src-tauri/target/release/bundle/nsis/）は単独配布しません。詳しくは setup/README.md を参照してください。
開発起動は npm.cmd run tauri:dev を使用し、正式版と設定領域を分離します。配布ビルドでは診断用featureを有効にしません。
テストは合成データと test-runtime/ の隔離領域を使用します。実利用設定をテストに使用しないでください。

## 配布・ライセンス

WinFormsの独自UIと無画面のインストールエンジンを使用し、ユーザー単位で導入します。WebView2未導入時はインターネット接続が必要です。
コード署名はありません。SmartScreen等の警告が表示される場合があります。正規配布元とSHA-256を確認してください。
素材・依存ライブラリのライセンスとソース入手先は、アプリ内の「ライセンス・クレジット」で確認できます。
全文はオフラインで閲覧できる1つの文書に集約しています。Urbanistの著作権表示とSIL OFL 1.1も含みます。

[v2.0.1 リリースノート](docs/RELEASE-NOTES-v2.0.1.md)

## 既知の制約

保存成否不明時の復旧・終了UX、戻らないCOM/WebView生成の安全な中断、長期メモリ収束には既知の残件があります。
