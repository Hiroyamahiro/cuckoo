# UI設定の管理場所

- メインウィンドウの初回サイズと最小サイズは `src-tauri/tauri.conf.json` の `app.windows` に定義する。`native_display.rs` はその設定を使ってDPI換算、位置保持、サイズ復元、220msの補間を行う。フロントからウィンドウ寸法を送信しない。作業領域を超える場合は画面内に収める。
- 色・フォントサイズ・共通変数は `src/theme.css`。背景画像が有効なときの下地色は `--forest-background`、通常の下地色は `--background`。トレイと本体は同じテーマ定義を読む。
- 基本フォントは `--font-family`。英数字は `src/fonts/urbanist/` の同梱Urbanist、日本語はWindows標準のYu Gothic UIへフォールバックする。Urbanistは100〜900の可変ウェイトを持ち、外部フォントサービスには接続しない。著作権とOFL全文は第三者ライセンス文書にも収録する。
- 電源ボタンの共通スタイルは `src/power-button.css`。タイトルバーは `src/ui-details.css`。高さは `--titlebar-height` を使い、本体の上余白もそこから求める。
- ダイヤルの表示上限は `--dial-max-size`。ミニマム表示では利用できる幅と高さにも合わせる。SVG内部の座標系は `dial-model.ts` の `dialSize` が基準で、表示サイズとは区別する。
- 音色選択の幅・最小幅・最大高さは `src/sound-select.css`。JavaScriptはボタン幅を `--trigger-width` で渡し、CSS適用後の実寸から画面内の配置を計算する。
- ライセンス全文とトレイパネルは別ウィンドウ。それぞれの生成処理にあるサイズは、メインウィンドウ設定の上書きではない。

`data-theme`、`data-forest`、`data-minimum`、`:hover`、`:disabled` などの状態別指定は意図した切り替え。通常表示と状態別表示の両方を確認する。JavaScriptによるダイヤル上の座標、ポップアップ位置、フェード中の位置・不透明度は動的な値なので残している。

開発中の画面とビルド済みexeを区別する。exeにはフロント資産が埋め込まれるため、CSSだけを編集しても再ビルド前のexeには反映されない。フロントビルドの失敗後にRustだけをビルドすると、古い `dist` を参照することがある。ライセンス生成文書が古い場合は `npm run licenses:generate` を実行してからビルドする。
