# CONTRIBUTING.md

> English version [below](#english).

# 中文

- 由于本项目是纯AI项目，人工审核较少，因此无法保证代码质量和统一性。我们建议使用AI进行代码编写和修改，并在完成后进行人工测试。
- 在贡献代码时请注意以下事项：
- 我们提供了 [CLAUDE.md](CLAUDE.md) 文件，该文件由人工编写，包含项目说明和贡献要求，请务必令AI按其要求进行工作。
    - 建议将其添加进系统提示词中，加强对AI的约束。
    - 如果你使用的是 AGENTS.md，请在其中添加 CLAUDE.md 的内容，或要求AI阅读，确保AI在编写代码时遵守要求。
    - 该文档主要提供给AI阅读，因此未使用英文。
    - 如果你的修改影响到了 CLAUDE.md 中的内容，请务必修改 CLAUDE.md 并人工验证以确保其内容与实际情况一致。
- 在 docs 目录下包含项目功能说明/指南和文档编写要求，在进行编写时建议先令AI阅读。docs/README.md 是该目录的说明和索引文档，建议先阅读。
- 请使用 Pull Request 的方式进行贡献，以便后续进行代码审查和测试。请在提交 PR 之前确保代码已经过测试，并且不会破坏已有功能。
- 请勿提交任何形式的广告或推广内容。
- 请勿在代码中添加 AI co-author、license、copyright 等信息。
- 请勿提交本地备份/临时文件/测试文件/调试文件等非必要文件。合理编写 .gitignore 文件，确保不会将这些文件提交到远程仓库。
- 在使用其他项目的代码时，注意其许可。本项目采用MIT许可。

# English

- Since this is a pure-AI project with little manual review, we cannot guarantee code quality or consistency. We recommend using AI for writing and modifying code, and performing manual testing after completion.
- Please note the following when contributing code:
- We provide a [CLAUDE.md](CLAUDE.md) file, which is written by humans and contains project information and contribution requirements. Please make sure the AI works according to its requirements.
    - We recommend adding it to the system prompt to strengthen the constraints on the AI.
    - If you use AGENTS.md, add the contents of CLAUDE.md to it, or ask the AI to read it, to ensure the AI follows the requirements when writing code.
    - This document is mainly provided for AI to read, so it is not written in English.
    - If your modifications affect the contents of CLAUDE.md, be sure to update CLAUDE.md and verify it manually so that its contents stay consistent with the actual state.
- The docs directory contains feature documentation/guides and documentation writing requirements. We recommend having the AI read it before writing. docs/README.md is the description and index document for this directory, and is recommended to be read first.
- Please contribute via Pull Request so that code review and testing can follow. Make sure the code has been tested and does not break existing functionality before submitting a PR.
- Do not submit any form of advertising or promotional content.
- Do not add AI co-author, license, copyright, or similar information to the code.
- Do not submit unnecessary files such as local backups, temporary files, test files, or debug files. Write a sensible .gitignore so these files are not committed to the remote repository.
- When using code from other projects, pay attention to their licenses. This project is licensed under the MIT license.