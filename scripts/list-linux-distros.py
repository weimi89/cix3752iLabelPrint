#!/usr/bin/env python3
"""列出 release workflow 會產出 tarball 的 Linux distro,每行一個。

供 release asset 驗收組出期望檔名用。distro 清單只在 release.yml 的 release-linux
matrix 維護一份,這裡解析同一份來源 —— 驗收若另抄一份清單,增減 distro 時兩邊會漂移,
漏驗的那個 distro 缺席也不會有人發現。

解析不到(欄位改名、job 更名、matrix 改寫法)一律以非 0 退出,不回傳空清單:
空清單會讓驗收「什麼都不用檢查」而靜默通過,比直接失敗更危險。
"""

import pathlib
import sys

import yaml

WORKFLOW = pathlib.Path(__file__).resolve().parent.parent / ".github/workflows/release.yml"


def main() -> int:
    if not WORKFLOW.exists():
        print(f"找不到 workflow 檔案:{WORKFLOW}", file=sys.stderr)
        return 1

    workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))

    try:
        include = workflow["jobs"]["release-linux"]["strategy"]["matrix"]["include"]
        distros = [item["distro"] for item in include]
    except (KeyError, TypeError) as exc:
        print(f"解析不到 release-linux 的 matrix distro 清單:{exc}", file=sys.stderr)
        return 1

    if not distros:
        print("release-linux 的 matrix distro 清單是空的", file=sys.stderr)
        return 1

    print("\n".join(distros))
    return 0


if __name__ == "__main__":
    sys.exit(main())
