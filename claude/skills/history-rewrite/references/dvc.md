# 書き換えの後の DVC

`dvc.lock` の出力の md5 は出力の中身で決まるので、語の書き換えでは変わらない。deps に書き換えたファイル（文書、
コメントを直したスクリプト）があるステージは `dvc status` で changed になり、過去のコミットの `dvc.lock` は書き換える
前の deps の md5 のまま残る。そのリポジトリの DVC の規則（xlc の `/dvc-relock`）は、走らせなかったステージの changed を
「そのままでよい」とするので、規則に任せず、書き換えのあとに次を行う（`D` は作業ディレクトリ）。

- (a) 書き換えが原因で changed になったステージを一覧にする。`dvc status` で deps が changed のステージ
  （`always_changed` のステージと、`changed outs` だけのステージは除く）のうち、書き換えの直前の HEAD で changed で
  なかったものを選ぶ。直前の HEAD の状態は、控えの original.git が bare なので `git clone "$D/original.git" <一時のパス>` で
  作業ツリーを作り、そのリポジトリの DVC の規則に従って出力を戻して（xlc なら `references` の symlink と
  `dvc checkout`、`build`）そこで `dvc status` を実行して得る
- (b) lock を付け直さないなら、対象のリポジトリに issue を開き、(a) の一覧と、下流のステージを流すときに上流が
  再実行される費用の見積もり（`dvc.yaml` のステージの依存と、記録された所要時間から）を書く
- (c) 付け直すなら、deps の差が出力を変えないことを、書き換えの前後の diff でステージごとに確かめる。文書の文言と
  コメントだけの差なら付け直す。文字列リテラル、ログの書式、ステージが読む語の差は出力を変えうるので (b) にする。
  `dvc commit -f <ステージ>` を実行する（`-f` が無いと確認を求めて止まる。出力は作り直さず、今の deps と outs の md5 を
  lock に書く）。lock は、そのリポジトリの DVC の規則の検査（xlc なら `scripts/check-dvc-lock.sh`）を通してコミットする。
  `dvc commit` は語の書き換えによる差にだけ使い、コードの変更があるステージは再実行する
