using System;
using System.Drawing;
using System.Threading.Tasks;
using System.Windows.Forms;
namespace CuckooSetup {
 public sealed class SmoothLogo:Control {
 readonly Image image;public SmoothLogo(Image source){image=(Image)source.Clone();DoubleBuffered=true;}
 protected override void OnPaint(PaintEventArgs e){base.OnPaint(e);e.Graphics.InterpolationMode=System.Drawing.Drawing2D.InterpolationMode.HighQualityBicubic;e.Graphics.PixelOffsetMode=System.Drawing.Drawing2D.PixelOffsetMode.HighQuality;e.Graphics.CompositingQuality=System.Drawing.Drawing2D.CompositingQuality.HighQuality;using(var a=new System.Drawing.Imaging.ImageAttributes()){a.SetWrapMode(System.Drawing.Drawing2D.WrapMode.TileFlipXY);e.Graphics.DrawImage(image,ClientRectangle,0,0,image.Width,image.Height,GraphicsUnit.Pixel,a);}}
 protected override void Dispose(bool disposing){if(disposing)image.Dispose();base.Dispose(disposing);}
 }
 public sealed class SetupForm:Form {
  readonly IBackend backend;readonly Flow flow;bool approvedExitShown;readonly bool uninstall; readonly Label heading,description,details;readonly Button primary,close;readonly ProgressBar progress;readonly CheckBox removeData;
  public SetupForm(IBackend backend,bool uninstall,string version){
   this.backend=backend;this.uninstall=uninstall;flow=new Flow(backend,uninstall);
   Text=uninstall?"Cuckoo のアンインストール":"Cuckoo セットアップ";ClientSize=new Size(520,360);AutoScaleDimensions=new SizeF(96,96);AutoScaleMode=AutoScaleMode.Dpi;
   FormBorderStyle=FormBorderStyle.FixedSingle;MaximizeBox=false;StartPosition=FormStartPosition.CenterScreen;BackColor=Color.FromArgb(250,250,248);ForeColor=Color.FromArgb(48,55,52);Font=new Font("Yu Gothic UI",10f);
   var layout=new TableLayoutPanel{Dock=DockStyle.Fill,Padding=new Padding(32,24,32,22),ColumnCount=1,RowCount=6};
   foreach(float h in new float[]{72,34,58,18,62,48})layout.RowStyles.Add(new RowStyle(SizeType.Absolute,h));Controls.Add(layout);
   var header=new Panel{Dock=DockStyle.Fill};using(var stream=typeof(SetupForm).Assembly.GetManifestResourceStream("Cuckoo.Icon")){if(stream!=null){using(var source=new Icon(stream)){Icon=(Icon)source.Clone();}}}
   using(var logoStream=typeof(SetupForm).Assembly.GetManifestResourceStream("Cuckoo.Logo")){using(var logo=Image.FromStream(logoStream)){header.Controls.Add(new SmoothLogo(logo){Size=new Size(48,48),Location=new Point(0,1)});}}
   header.Controls.Add(new Label{Text="Cuckoo",Font=new Font("Segoe UI",22f),AutoSize=true,Location=new Point(62,-2)});header.Controls.Add(new Label{Text="Version "+version,Font=new Font("Segoe UI",9f),ForeColor=Color.DimGray,AutoSize=true,Location=new Point(65,40)});layout.Controls.Add(header,0,0);
   heading=new Label{Dock=DockStyle.Fill,Font=new Font("Yu Gothic UI",13f,FontStyle.Bold)};layout.Controls.Add(heading,0,1);
   description=new Label{Dock=DockStyle.Fill};layout.Controls.Add(description,0,2);
   progress=new ProgressBar{Dock=DockStyle.Top,Height=4,Style=ProgressBarStyle.Marquee,MarqueeAnimationSpeed=25,Visible=false};layout.Controls.Add(progress,0,3);
   details=new Label{Dock=DockStyle.Fill,Font=new Font("Yu Gothic UI",8.5f),ForeColor=Color.FromArgb(173,52,52),AutoEllipsis=true};var feedback=new Panel{Dock=DockStyle.Fill};details.Height=32;details.Dock=DockStyle.Bottom;feedback.Controls.Add(details);removeData=new CheckBox{Text="設定と追加した音源も削除する",AutoSize=true,Location=new Point(0,0),Visible=uninstall};feedback.Controls.Add(removeData);layout.Controls.Add(feedback,0,4);
   var buttons=new FlowLayoutPanel{Dock=DockStyle.Fill,FlowDirection=FlowDirection.RightToLeft,WrapContents=false};primary=new Button{Width=180,Height=38,FlatStyle=FlatStyle.Flat,BackColor=Color.FromArgb(30,128,74),ForeColor=Color.White};primary.FlatAppearance.BorderSize=0;close=new Button{Text="閉じる",Width=80,Height=38,FlatStyle=FlatStyle.Flat};close.FlatAppearance.BorderColor=Color.FromArgb(226,229,224);buttons.Controls.Add(primary);buttons.Controls.Add(close);layout.Controls.Add(buttons,0,5);
   primary.Click+=async(s,e)=>{if(flow.State==Phase.Complete){if(uninstall)Close();else{await flow.Launch();if(flow.Error==null)Close();}}else {var options=backend as IRemovalOptions;if(uninstall&&removeData.Checked&&MessageBox.Show(this,"設定と追加した音源を完全に削除します。元に戻せません。","Cuckoo",MessageBoxButtons.OKCancel,MessageBoxIcon.Warning)!=DialogResult.OK)return;if(options!=null)options.RemoveUserData=uninstall&&removeData.Checked;var update=backend as IUpdateOptions;if(!uninstall&&update!=null&&update.AppRunning&&!approvedExitShown){Render();return;}await flow.Run(approvedExitShown);if(flow.Restarted)Close();}};close.Click+=(s,e)=>Close();
   FormClosing+=(s,e)=>{if(!flow.CanClose){e.Cancel=true;description.Text="処理の終了を確認しています。このままお待ちください。";}};
   var timer=new System.Windows.Forms.Timer{Interval=1000};timer.Tick+=(s,e)=>{if(flow.State==Phase.Unconfirmed||flow.State==Phase.Ready||flow.State==Phase.Failed)Render();};timer.Start();FormClosed+=(s,e)=>timer.Dispose();
   flow.Changed+=()=>{if(IsHandleCreated&&!IsDisposed)BeginInvoke((Action)Render);};Shown+=(s,e)=>Render();Render();
  }
  void Render(){bool working=flow.State==Phase.Working||flow.State==Phase.Launching;progress.Visible=working;primary.Enabled=!working;close.Enabled=flow.CanClose;details.Text="";removeData.Enabled=flow.State==Phase.Ready||flow.State==Phase.Failed;removeData.Visible=uninstall&&(flow.State==Phase.Ready||flow.State==Phase.Failed);
   switch(flow.State){
    case Phase.Ready:heading.Text=uninstall?"Cuckooを削除します":backend.IsUpdate?"Cuckooを更新します":"毎日の時報を、シンプルに。";description.Text=uninstall?"アプリとショートカットを削除します。\n設定と追加した音源は残ります。":backend.RuntimeMissing?"起動に必要なWebView2も導入します。\nインターネット接続が必要です。":backend.IsUpdate?"設定と追加した音源は保持します。":"このユーザーのPCにインストールします。";primary.Text=uninstall?"アンインストール":backend.IsUpdate?"更新":"インストール";break;
    case Phase.Working:heading.Text=uninstall?"削除しています":"準備しています";description.Text=flow.Detail??"しばらくお待ちください。";primary.Text="処理中";break;
    case Phase.Complete:heading.Text=uninstall?"削除が完了しました":"準備ができました";description.Text=uninstall?(removeData.Checked?"設定と追加した音源も削除しました。":"設定と追加した音源は保持しています。"):"スタートメニューのCuckooから起動できます。";primary.Text=uninstall?"閉じる":"Cuckooを起動";close.Visible=!uninstall;break;
    case Phase.Launching:heading.Text="Cuckooを起動しています";description.Text="しばらくお待ちください。";primary.Text="起動中";break;
    case Phase.Failed:heading.Text="処理を完了できませんでした";description.Text=flow.Error;details.Text="確認後に再試行してください。\nログ："+backend.LogPath;primary.Text="再試行";break;
    case Phase.Unconfirmed:heading.Text="結果を確認できませんでした";description.Text="再実行せず、ログを確認してください。\n設定や追加した音源は削除しないでください。";details.Text=flow.Error+"\nログ："+backend.LogPath;primary.Enabled=false;primary.Text="確認が必要";break;
   }
   if(!uninstall&&(flow.State==Phase.Ready||flow.State==Phase.Failed)){var update=backend as IUpdateOptions;approvedExitShown=update!=null&&update.AppRunning;if(approvedExitShown){heading.Text=flow.State==Phase.Failed?"更新を保留しています":"Cuckooを終了して更新します";description.Text=flow.State==Phase.Failed?flow.Error:"終了を確認してから更新します。設定・プリセット・追加した音源は保持します。";primary.Text="終了して更新";}}
   if(flow.State==Phase.Complete&&flow.Error!=null)details.Text=flow.Error;
  }
 }
}

