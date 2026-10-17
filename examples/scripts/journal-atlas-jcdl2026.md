# Pitch
! The script the author used at the JCDL 2026 online demo session (public parts): a one-minute pitch, then prepared answers.
! （slide：標題）
Hi, I'm Meng-Han Lee, an independent researcher from Taiwan. Journal Atlas is an open-source GitHub database of journals' soft metadata: the tacit knowledge no index records. It runs as an agent skill, and anyone can contribute.
> 大家好，我是 Meng-Han Lee，來自台灣的獨立研究者。Journal Atlas 是放在 GitHub 上的開源資料庫，收錄期刊的「軟性資訊」：任何索引都沒記錄的隱性知識。它以 agent skill 的形式執行，任何人都能貢獻。

! （slide：三行紫字）
Papers get rejected for exactly that: fees, word limits, ethics approval, how reviewers treat your method.
> 論文常常就是因為這些被拒：費用、字數上限、倫理審查、審稿人怎麼看待你的研究方法。

An AI does the first pass, with a source link for every fact. People add what only people know. And every score shows how much evidence it rests on.
> AI 先做第一輪，每個事實都附來源連結。只有人知道的事，由人補上。每個分數都會顯示它背後有多少證據。

! （slide：黑色程式碼框、右下 QR —— 手指向畫面右下）
Two commands install it on Claude or Codex. Details and a live demo in the breakout room. Thank you.
> 在 Claude 或 Codex 上兩行指令就能安裝。細節和現場示範在分組討論室。謝謝。

## 難念的字
! venue = VEN-yoo · metadata = MET-uh-day-tuh · tacit = TASS-it · ethics = ETH-iks · evidence = EV-ih-dens · contribute = kun-TRIB-yoot

# Q&A
! Answers are taken from the paper and the repository, and kept deliberately modest. For a question not prepared here, type the answer in Chinese in the Reply window.

Q: How is this different from B!SON, Jane, eTBLAST?
> 問：這跟 B!SON、Jane、eTBLAST 有什麼不同？
They match your abstract to published articles, so they find topically similar venues. I add what no index records — fees, word limits, ethics approval, reviewer culture — and every score says how much evidence is behind it.
> 它們拿你的摘要去比對已發表的文章，所以找到的是主題相近的期刊。我補上的是索引不記錄的東西——費用、字數上限、倫理審查、審稿文化——而且每個分數都標明背後有多少證據。

Q: Who checked the AI-collected entries?
> 問：AI 蒐集的資料，有誰檢查過？
Not humans yet. 236 of the 399 entries are AI-researched and marked that way. I audited 30 random claim–source pairs: 12 sources could not be read automatically; of the 18 that could, 15 supported the claim, 1 partly, and 2 did not. So a source link is not proof, which is why the tiers exist.
> 還沒有人工檢查。399 筆裡有 236 筆是 AI 查的，也都標成 AI 查的。我隨機抽查了 30 組「宣稱—來源」：12 個來源無法自動讀取；能讀的 18 個裡，15 個支持宣稱、1 個部分支持、2 個不支持。所以有來源連結不等於證明，這就是要分層級的原因。

Q: Does it work for my field?
> 問：我的領域能用嗎？
Today 88% of entries are in psychology, philosophy, HCI and cognitive science, and the page says so before you type. If your field is thin, that is where contributions help.
> 目前 88% 的資料在心理學、哲學、HCI 和認知科學，頁面在你輸入之前就會說明。如果你的領域資料少，那正是需要大家貢獻的地方。

Q: Why a skill, not just a website?
> 問：為什麼做成 skill，而不只是網站？
The skill is Markdown files plus scripts. It runs in your own AI session on your own key. There is no server of ours in the loop, so if the demo site disappears the knowledge base still works.
> skill 就是 Markdown 檔加上腳本，在你自己的 AI session、用你自己的 key 執行。中間沒有經過我們的伺服器，所以就算示範網站消失，知識庫照樣能用。

Q: How do I contribute?
> 問：要怎麼貢獻？
In Claude Code, /ja-contribute interviews you about a journal and writes a patch you send as a pull request. To be honest: there are no outside contributors yet.
> 在 Claude Code 裡，/ja-contribute 會訪談你對某本期刊的了解，寫成一個 patch，你再用 pull request 送出。老實說：目前還沒有外部貢獻者。

Q: Did you test whether it improves acceptance?
> 問：有測過它能不能提高錄取率嗎？
No. I make no claim about acceptance outcomes; I have not measured them. The claim is about the decision problem and how legible the output is.
> 沒有。我不對錄取結果做任何宣稱，我沒有量測過。我的主張是關於這個決策問題本身，以及輸出結果有多容易看懂。

Q: What is "evidence" in "100% evidence"?
> 問：「100% evidence」裡的 evidence 指什麼？
The share of the six scoring dimensions that have recorded data behind them. A score resting on little evidence is shrunk toward neutral, and missing evidence can never raise a score.
> 指六個評分維度中，有實際紀錄資料支撐的比例。證據少的分數會被往中間值收縮；缺少證據永遠不會讓分數變高。

Q: License and citation?
> 問：授權和引用方式？
Corpus CC BY-SA 4.0, code MIT. Paper: doi 10.1145/3805696.3846515 (ACM DL). Archived version: Zenodo doi 10.5281/zenodo.22272331.
> 資料庫用 CC BY-SA 4.0，程式碼用 MIT。論文 DOI 10.1145/3805696.3846515（ACM DL）。存檔版本：Zenodo DOI 10.5281/zenodo.22272331。
```copy:cite
Paper: https://doi.org/10.1145/3805696.3846515
Archived: https://doi.org/10.5281/zenodo.22272331
Corpus CC BY-SA 4.0 · Code MIT
```

## 救急句
Sorry, could you repeat the question?
> 抱歉，可以請你再說一次問題嗎？
Could you say a bit more about what you mean?
> 可以多說一點你的意思嗎？
That's a good question. I haven't measured that, so I don't want to guess.
> 好問題。這個我沒有量測過，所以不想用猜的。
I'll put the link in the chat.
> 我把連結貼到聊天室。
