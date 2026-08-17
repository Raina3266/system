"""Fold colour schemes and other KConfig fragments into ~/.config/kdeglobals.

KColorScheme builds the palette from kdeglobals itself; a .colors file in
share/color-schemes is only consulted when kdeglobals has no [Colors:View]
group (see KHintsSettings::loadPalettes in plasma-integration).  Applying a
scheme therefore means copying its groups *into* kdeglobals, which is what the
Colors KCM does and what this script does at activation time.

kdeglobals stays a normal writable file, because KIO and friends persist their
own state in it.  Colour groups are owned by the sources and replaced whole;
every other group is merged key by key so that state survives.

Usage: merge-kdeglobals.py TARGET SOURCE...
"""

import configparser
import os
import sys
import tempfile

OWNED_PREFIXES = ("Colors:", "ColorEffects:")
OWNED_GROUPS = ("WM",)


def owned(section):
    """Groups this config owns outright, rather than merging into."""
    return section.startswith(OWNED_PREFIXES) or section in OWNED_GROUPS


def parser():
    # KConfig keys are case sensitive, values may contain '%' and ':', and
    # duplicate keys show up in files apps have rewritten.
    p = configparser.RawConfigParser(
        strict=False, delimiters=("=",), interpolation=None
    )
    p.optionxform = str
    return p


def read(path):
    p = parser()
    with open(path, encoding="utf-8") as fh:
        p.read_file(fh)
    return p


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__.strip().splitlines()[-1])

    # abspath so that dirname() is usable for a bare filename too.
    target, sources = os.path.abspath(argv[1]), argv[2:]

    merged = parser()
    if os.path.exists(target):
        merged = read(target)

    # Drop every owned group first so renamed or deleted entries cannot linger
    # from an earlier generation.
    for section in [s for s in merged.sections() if owned(s)]:
        merged.remove_section(section)

    for path in sources:
        source = read(path)
        for section in source.sections():
            if owned(section):
                merged[section] = dict(source[section])
                continue
            if not merged.has_section(section):
                merged.add_section(section)
            for key, value in source[section].items():
                merged.set(section, key, value)

    # Removing and re-adding groups reorders them, which would make the file
    # churn on every run.  Emit groups sorted so the output is stable.
    ordered = parser()
    for section in sorted(merged.sections()):
        ordered[section] = dict(merged[section])

    directory = os.path.dirname(target)
    os.makedirs(directory, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=directory)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            ordered.write(fh, space_around_delimiters=False)
        os.chmod(tmp, 0o644)
        os.replace(tmp, target)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


if __name__ == "__main__":
    main(sys.argv)
