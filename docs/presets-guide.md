# 预设系统新手教程 / Presets Guide for Beginners

> 本教程面向零基础用户，从零开始讲解预设系统的每一个按钮和概念。
> English version [below](#english).

# 中文

## 1. 什么是预设？

预设（Preset）是一整套插件设置的"快照"。插件有几十项设置（卡片内容、算法、主题、功能开关……），手动一项项调整很麻烦。预设可以：

- **一键应用**：选一个预设，所有设置立刻变成预设里保存的样子。
- **一键保存**：把当前调好的配置存成一个预设，以后随时恢复。
- **分享给朋友**：把预设导出成文件，朋友导入即可使用。

插件自带 11 个内置预设（系统预设，只读），你也可以创建任意多个自定义预设。

## 2. 如何打开预设管理器？

预设管理器是一个独立的网页页面，位于插件目录内：

1. 在浏览器地址栏输入：`http://localhost:24050/<插件目录名>/presets.html`
   - 例如插件目录名为 `ManiaMapAnalyser by Leo_Black`，则访问 `http://localhost:24050/ManiaMapAnalyser%20by%20Leo_Black/presets.html`
   - 如果修改过端口，把 `24050` 换成你的端口
2. 页面加载后，你看到的就是预设管理器。

> 提示：这个页面只是管理工具，不影响游戏内显示的叠加界面。预设应用后，游戏内的显示会立即更新（需要 tosu 正在运行）。

## 3. 界面总览

预设管理器分为三个部分：

- **顶部操作栏**（蓝色横条）：全局操作按钮，如新建、保存、应用、导出、导入。
- **左侧列表**：所有预设的列表，分为 **System**（系统预设）和 **My Presets**（我的预设）两组。
- **右侧编辑区**：上方是 **Preset Info**（预设信息），下方是**设置表单**（所有可配置项，每项前面有一个复选框）。

> 提示：刚打开或刷新页面时，My Presets 会先显示"Loading presets…"，等 tosu 的设置数据到达后（通常 1~2 秒）才会显示真实的预设列表——这不是丢失，请稍候。

## 4. 左侧列表详解

### System（系统预设）

系统预设由插件自带，**不能删除**。点击应用即可使用。

- **Default**：恢复出厂设置。应用后所有设置回到插件默认值。
- 11 个内置预设，例如：
  - **Mini**：极简模式，只显示星数。
  - **ForOsuPlayer / ForEtternaPlayer / ForInterludePlayer**：面向不同玩家的推荐配置。
  - **PatternFocus / FullOverview / VibroPlayer / JackPlayer**：面向不同游玩场景。
  - **TheLimitDoesNotExist / DanielLike / WildDanWIP**：面向高难与特殊玩法。
- **LastSavedPreset**（只读行）：它不是可应用的预设，而是"自动跟随"标记——选择它之后，你在 tosu 设置页手动修改的设置会自动保存到这里，永远保留你最后一次的手动配置。

每个预设行都有按钮：

| 按钮 | 作用 |
| --- | --- |
| **Edit** | 把该预设加载到右侧编辑区（名称、说明、设置值），你可以查看它的具体配置、修改后另存为自定义预设 |
| **Apply** | 一键应用该预设，游戏内叠加界面立即生效 |
| （自定义预设）**Rename** | 重命名（需要符合命名规则） |
| （自定义预设）**Delete** | 删除该预设（需要确认） |
| （自定义预设）**Export** | 导出该预设为 .json 文件 |

行首的 **SYSTEM** 徽章表示系统预设；名称旁的数字如 `v1` 是预设版本号。

### My Presets（我的预设）

你创建的预设都在这里。首次使用时会自动创建 **Custom1 / Custom2 / Custom3** 三个固定槽位（它们不可以重命名或删除，但可以应用和覆盖保存内容）。之后你可以创建任意多个自定义预设。

## 5. 右侧编辑区详解

### Preset Info（预设信息）

编辑区顶部是预设的元信息：

- **Name（名称）**：预设名字。规则：只能使用英文字母、数字、下划线 `_` 和连字符 `-`，最长 40 个字符（例如 `my_osu_preset`）。
- **Description（说明）**：一句话描述这个预设的用途（最长 200 字符）。
- **Version（版本）**：纯数字，越大代表越新。每次修改后可以手动 +1 标记新版本。
- **ID (auto)**：自动生成的内部标识，无需手动填写（由名称自动生成）。

### 设置表单

表单列出了插件**所有**可配置设置（自动从 settings.json 生成，新增设置会自动出现）。每一项前面有一个**复选框**：

- **勾选** = 该设置会包含在预设中（保存/应用时生效）
- **不勾选** = 该设置不包含在预设中（应用时保持当前值不变）

这就是"部分预设"：你只需要勾选想管理的设置，其余设置应用时不会被改动。

> 注意：表单中不显示 `Preset` 和 `Preset Storage` 两个系统内部设置；`WebSocket Endpoint` 默认不勾选（它是连接参数，一般不应包含在预设里）。

表单值会跟随 tosu 的实时广播自动刷新（正在输入的那一项除外）。

## 6. 顶部操作栏逐个按钮讲解

| 按钮 | 作用 | 注意事项 |
| --- | --- | --- |
| **New** | 清空编辑区，开始创建新预设 | 会弹出确认框；当前编辑内容会被清掉 |
| **Save** | 把编辑区的设置（勾选项）+ 预设信息保存为预设 | 如果名字已存在，会弹出确认框询问是否覆盖；名字必须符合命名规则 |
| **Apply Checked** | 把编辑区**勾选**的设置立即应用到插件 | 会弹出确认框；未勾选的设置不受影响 |
| **Export Current** | 把编辑区当前内容（勾选项 + 预设信息）导出为 .json 文件 | 用于分享或备份当前正在编辑的预设 |
| **Export All** | 把所有自定义预设导出为一个 .json 文件 | 用于整体备份或分享 |
| **Import** | 导入 .json 预设文件 | 选择文件后自动导入；格式错误会提示 |

### 推荐工作流程

1. **创建预设**：在表单里调整设置（勾选想要的项）→ 填写 Preset Info → 点 **Save**。
2. **应用预设**：在左侧列表点某行的 **Apply**（或编辑后点 **Apply Checked**）。
3. **修改预设**：列表点 **Edit** → 修改表单/信息 → 点 **Save**（覆盖）。
4. **分享预设**：列表点某行的 **Export**（单个）或操作栏 **Export All**（全部）→ 把 .json 文件发给朋友 → 朋友点 **Import** 导入。

## 7. 常见问题

**Q: 应用预设后哪些设置会变？**
A: 只变预设里包含（勾选）的设置。自定义预设只保存你勾选的字段；系统预设保存它设计好的字段；未包含的设置保持当前值。

**Q: 预设存在哪里？换电脑会丢吗？**
A: 自定义预设存在 tosu 的设置文件里（`presetStorage` 设置项），随 tosu 的 settings 目录一起保存。换电脑时拷贝 tosu 的 `settings` 目录即可带走。也可以随时用 **Export All** 导出备份。

**Q: 为什么游戏内显示没变化？**
A: 请确认 tosu 正在运行、且浏览器访问的端口与 tosu 一致。应用成功后页面右上角会弹出绿色提示。

**Q: 为什么我的预设名保存失败？**
A: 名字只能包含英文字母、数字、`_` 和 `-`（最长 40 字符），且不能与系统预设重名，不能叫 `Custom` 或 `LastSavedPreset`。

**Q: Custom1/2/3 能删除吗？**
A: 这三个是固定槽位，不能删除/重命名，但可以随时把新配置保存进去覆盖内容。

**Q: 导出的文件能直接改吗？**
A: 可以，文件是纯文本 JSON 格式，用记事本打开即可查看；但请小心不要破坏格式，改坏了导入时会提示错误。

---

# English

## 1. What is a preset?

A preset is a snapshot of the plugin's settings. The plugin has dozens of settings (card content, algorithm, theme, toggles...). A preset lets you:

- **Apply in one click**: pick a preset and all settings change to the saved values instantly.
- **Save in one click**: store your current configuration as a preset and restore it later.
- **Share with friends**: export a preset to a file; friends import it and get the same configuration.

The plugin ships with 11 built-in presets (read-only system presets). You can also create any number of custom presets.

## 2. How do I open the Presets Manager?

The manager is a standalone page inside the plugin folder:

1. Open your browser and go to: `http://localhost:24050/<plugin folder name>/presets.html`
   - E.g. `http://localhost:24050/ManiaMapAnalyser%20by%20Leo_Black/presets.html`
   - If you changed the port, replace `24050` with your port.
2. The page that loads is the Presets Manager.

> Note: this page is only a management tool. Applying a preset updates the in-game overlay immediately (tosu must be running).

## 3. Interface overview

The manager has three parts:

- **Action bar** (top): global actions — New, Save, Apply, Export, Import.
- **List** (left): all presets, grouped into **System** and **My Presets**.
- **Editor** (right): **Preset Info** on top and the **settings form** below (every setting with a checkbox).

> Note: right after opening or refreshing the page, My Presets shows "Loading presets…" until tosu's settings data arrives (usually 1–2 seconds) — your presets are not lost, just wait a moment.

## 4. The list in detail

### System presets

Built into the plugin and **cannot be deleted**.

- **Default**: factory reset — applies the plugin's default values to everything.
- 11 built-in presets, e.g. **Mini** (star rating only), **ForOsuPlayer / ForEtternaPlayer / ForInterludePlayer** (recommended configs per player type), **PatternFocus / FullOverview / VibroPlayer / JackPlayer** (per play style), **TheLimitDoesNotExist / DanielLike / WildDanWIP** (high-difficulty / niche).
- **LastSavedPreset** (read-only row): not an applicable preset but a "follow mode" marker — while selected, your manual changes in the tosu settings page are saved into it automatically, keeping your latest manual configuration.

Buttons on each row:

| Button | What it does |
| --- | --- |
| **Edit** | Loads the preset into the editor (name, description, values) — inspect it, tweak it, and save as a new custom preset |
| **Apply** | Applies the preset to the overlay immediately |
| (custom) **Rename** | Renames the preset (must follow the naming rules) |
| (custom) **Delete** | Deletes the preset (asks for confirmation) |
| (custom) **Export** | Downloads the preset as a .json file |

The **SYSTEM** badge marks system presets; a `v1`-style label next to the name is the preset version.

### My Presets

Your own presets. On first use the plugin auto-creates three fixed slots **Custom1 / Custom2 / Custom3** (they cannot be renamed or deleted, but their content can be overwritten). After that you can create unlimited custom presets.

## 5. The editor in detail

### Preset Info

- **Name**: English letters, digits, underscore `_` or hyphen `-` only, max 40 chars (e.g. `my_osu_preset`).
- **Description**: one line about what this preset is for (max 200 chars).
- **Version**: a whole number; higher means newer. Bump it after edits if you like.
- **ID (auto)**: auto-generated internal identifier — do not type it manually.

### Settings form

The form lists **every** configurable setting (auto-generated from settings.json — new settings appear automatically). Each row has a checkbox:

- **Checked** = included in the preset (applied on save/apply).
- **Unchecked** = not included — the setting keeps its current value when the preset is applied.

This is the "partial preset" feature: check only what you want to manage.

> Note: the internal `Preset` and `Preset Storage` settings are hidden from the form; `WebSocket Endpoint` is shown but unchecked by default (it is a connection parameter).

Form values auto-refresh from tosu's live broadcast (except the field you are typing in).

## 6. Action bar buttons, one by one

| Button | What it does | Notes |
| --- | --- | --- |
| **New** | Clears the editor to start a new preset | Asks for confirmation |
| **Save** | Saves the checked settings + Preset Info as a preset | Asks before overwriting an existing name; name must follow the rules |
| **Apply Checked** | Applies only the checked settings immediately | Asks for confirmation; unchecked settings stay untouched |
| **Export Current** | Downloads the current editor state (checked settings + info) as .json | For sharing/backing up what you are editing |
| **Export All** | Downloads all custom presets as one .json file | For full backup or sharing |
| **Import** | Imports a .json preset file | Errors are reported if the format is wrong |

### Recommended workflow

1. **Create**: tweak the form (check the fields you want) → fill Preset Info → click **Save**.
2. **Apply**: click **Apply** on a row (or **Apply Checked** after editing).
3. **Edit**: click **Edit** → modify → **Save** (overwrites).
4. **Share**: click **Export** on a row (single) or **Export All** (everything) → send the .json → friend clicks **Import**.

## 7. FAQ

**Q: Which settings change when I apply a preset?**
A: Only the settings included (checked) in the preset. Custom presets store only what you checked; system presets store their designed fields; anything not included keeps its current value.

**Q: Where are presets stored? Will I lose them on a new PC?**
A: Custom presets live in tosu's settings file (the `presetStorage` setting), inside tosu's `settings` folder — copy that folder to move them. Or use **Export All** anytime for a backup.

**Q: Nothing changed in-game?**
A: Make sure tosu is running and the port matches. A green toast appears in the top-right corner after a successful apply.

**Q: Why can't I save my preset name?**
A: Names may contain only English letters, digits, `_` and `-` (max 40 chars), must not clash with system presets, and must not be `Custom` or `LastSavedPreset`.

**Q: Can I delete Custom1/2/3?**
A: They are fixed slots — not deletable/renamable, but you can overwrite their content anytime.

**Q: Can I edit an exported file?**
A: Yes, it is plain JSON — open it in any text editor. Be careful not to break the format, or the import will report an error.
