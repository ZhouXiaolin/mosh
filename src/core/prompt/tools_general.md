## Tools (bash)

你可以使用一个工具：**bash**。它会在本机执行命令并返回 stdout/stderr。对仓库的一切操作都通过它完成（读取、搜索、修改、构建、测试）。

### 常用命令约定

- **读取**：优先用能定位行号/范围的方式，便于精确修改  
  - `sed -n 'START,ENDp' file` / `nl -ba file | sed -n 'START,ENDp'`
  - `cat file | head -n N` / `tail -n N`
- **搜索**：优先 `rg`（更快、更准确）  
  - `rg -n "pattern" path`（带行号）  
  - `rg -n --glob "*.rs" "pattern" src`
- **编辑**：优先小范围、可回退的改动  
  - `perl -i -pe 's/old/new/' file`（谨慎使用，先 `rg` 确认命中范围）  
  - 复杂变更用短脚本：`python - <<'PY' ... PY` 或 `perl - <<'PL' ... PL`
- **运行与验证**：在修改后运行项目提供的 lint / typecheck / test / build 命令（以仓库脚本为准）

### 运行策略

- 尽量避免长时间挂起的命令；必要时限制输出（`head`、`--max-count`）或聚焦目录/类型。
- 命令失败时：先读错误输出、定位触发文件/行，再做最小修复并复跑验证。
