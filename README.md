# Cuckoo

毎日の時報を、シンプルに。好きなチャイムやVOICEVOXの声で時刻を知らせる、Windows向けの時報アプリです。

## ダウンロード

> ### [Windows版をダウンロード](https://github.com/Hiroyamahiro/cuckoo/releases/download/v2.1.0/Cuckoo_2.1.0_x64-setup.exe)
>
> **v2.1.0 · Windows 64ビット版** — ダウンロードしたEXEを開いてインストールしてください。
>
> [最新Release・更新内容](https://github.com/Hiroyamahiro/cuckoo/releases/latest) · [SHA-256チェックサム](https://github.com/Hiroyamahiro/cuckoo/releases/download/v2.1.0/SHA256SUMS.txt)

## 主な機能

- **好きな時刻に時報**：毎時00分・30分の設定に加え、1分単位でカスタム時報を追加できます。ダイヤルからクリック・ドラッグでON／OFFできます。
- **選べる音色**：鳩時計・教会ベル・振り子時計・学校のチャイムを内蔵。自分のMP3も追加・試聴・名前変更・登録解除ができます。
- **VOICEVOXの音声時報**：キャラクターとスタイルを選び、時刻や自由なセリフを読み上げます。生成済み音声はVOICEVOXを終了しても再生できます。
- **音量・回数の調整**：全体と時報ごとの音量、チャイムの再生回数を設定できます。音声読み上げは1回固定です。
- **まとめて設定**：00分・30分を個別に操作できる「一括設定」と、48枠のON／OFFを保存するプリセットを利用できます。
- **控えめな表示**：ライト／ダーク、ミニマム表示、タスクトレイへの格納に対応しています。
- **アプリ内で更新**：新しい正式版を確認・ダウンロードできます。更新時も設定・プリセット・追加音源を引き継ぎます。

音声の初回生成には、別途VOICEVOXまたはVOICEVOX Engineの導入・起動が必要です。CuckooにVOICEVOX本体や音声モデルは同梱していません。

## 利用ガイド

[インストール・使い方を見る](docs/USER-GUIDE-ja.md)

[v2.1.0の更新内容](docs/RELEASE-NOTES-v2.1.0.md)

製作者：ひろやまひろし / Hiroyama Hiroshi

## 開発者向け情報

### 開発・テスト・ビルド

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

### 実装・設定の管理

- Rustが時刻・音声再生・トレイ操作を管理します。トレイ格納時はWebViewを破棄し、復帰時に再生成します。
- 00分・30分の時報は48枠で管理し、音色と1回／2回の再生回数を個別に設定します。VOICEVOXのキャラクター・スタイルは接続先から動的に取得します。
- 起動時と約24時間ごとに安定版の更新を確認します。取得したインストーラーをSHA-256で照合し、独自UIのインストーラーで承認後に更新・再起動します。
- v8設定を専用領域に保存・バックアップします。旧WAV素材と再生経路は削除済みで、旧選択はバックアップ後に鳩時計MP3へ移行します。
- 共通のカスタムセレクトとローカルSVGを使用しています。タイトルバーは簡素化した独自UIです。
- 英数字フォントUrbanistをローカル同梱し、日本語にはWindows標準フォントを使用します。フォント取得のための通信は行いません。

正式版の識別子は `jp.hiroyamahiroshi.cuckoo` です。開発版・Electron版の設定は自動で読み込まず、正式版専用の設定を使用します。正式版の更新時は既存の正式版設定を維持します。

## 配布・ライセンス

WinFormsの独自UIと無画面のインストールエンジンを使用し、ユーザー単位で導入します。WebView2未導入時はインターネット接続が必要です。
コード署名はありません。SmartScreen等の警告が表示される場合があります。正規配布元とSHA-256を確認してください。
素材・依存ライブラリのライセンスとソース入手先は、アプリ内の「ライセンス・クレジット」で確認できます。
全文はオフラインで閲覧できる1つの文書に集約しています。Urbanistの著作権表示とSIL OFL 1.1も含みます。

## 既知の制約

保存成否不明時の復旧・終了UX、戻らないCOM/WebView生成の安全な中断、長期メモリ収束には既知の残件があります。
