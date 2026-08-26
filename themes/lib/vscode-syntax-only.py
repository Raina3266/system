# Write a copy of a VS Code colour theme with its workbench colours removed,
# leaving only the syntax highlighting rules. VS Code then falls back to its
# own defaults for every workbench colour, so the editor keeps its normal
# look and only the code is recoloured.
#
# Theme files are JSONC — comments and trailing commas are allowed, and
# json.loads accepts neither — so strip both before parsing.
import json
import re
import sys


def strip_comments(text):
    out = []
    i = 0
    in_string = False
    escaped = False
    while i < len(text):
        c = text[i]
        if in_string:
            out.append(c)
            if escaped:
                escaped = False
            elif c == "\\":
                escaped = True
            elif c == '"':
                in_string = False
            i += 1
        elif c == '"':
            in_string = True
            out.append(c)
            i += 1
        elif c == "/" and text[i + 1 : i + 2] == "/":
            while i < len(text) and text[i] != "\n":
                i += 1
        elif c == "/" and text[i + 1 : i + 2] == "*":
            end = text.find("*/", i + 2)
            i = len(text) if end < 0 else end + 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


source, destination, name = sys.argv[1:4]

text = strip_comments(open(source).read())
text = re.sub(r",(\s*[}\]])", r"\1", text)  # trailing commas

theme = json.loads(text)
theme.pop("colors", None)
theme["name"] = name

with open(destination, "w") as handle:
    json.dump(theme, handle, indent=2)
    handle.write("\n")
