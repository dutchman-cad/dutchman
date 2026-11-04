#!/usr/bin/env python3
"""Stage installed guidance and optionally generate a WiX fragment for those exact files."""
import argparse
import hashlib
import uuid
from pathlib import Path
import shutil
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
# Per-user components keep an HKCU value as key path, so WiX cannot generate their GUIDs.
# Each installed path gets a stable GUID derived from it, so upgrades keep the same component.
GUID_NAMESPACE = uuid.UUID("46db5fe6-d099-4a7d-ad78-9d644d1d01b6")
IMAGE_LIMIT = 256 * 1024


def stage(destination: Path, root: Path = ROOT) -> list[Path]:
    destination.mkdir(parents=True, exist_ok=True)
    sources = [(root / "assets/install/AGENTS.md", Path("AGENTS.md"))]
    for path in sorted((root / "skills").rglob("*")):
        if path.is_file():
            sources.append((path, path.relative_to(root)))
    for path in sorted((root / "docs").rglob("*")):
        if not path.is_file():
            continue
        image = path.suffix.lower() in (".png", ".jpg", ".jpeg", ".svg", ".webp", ".gif")
        if path.suffix.lower() == ".md" or (image and path.stat().st_size <= IMAGE_LIMIT):
            sources.append((path, path.relative_to(root)))
    for source, relative in sources:
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    return [relative for _, relative in sources]


def wix_fragment(destination: Path, files: list[Path], output: Path) -> None:
    def ident(prefix, relative):
        return prefix + hashlib.sha256(relative.as_posix().encode()).hexdigest()[:20]

    wix = ET.Element("Wix", xmlns="http://wixtoolset.org/schemas/v4/wxs")
    fragment = ET.SubElement(wix, "Fragment")
    parent = ET.SubElement(fragment, "DirectoryRef", Id="INSTALLFOLDER")
    group = ET.SubElement(fragment, "ComponentGroup", Id="GuideFiles")
    directories = {Path("."): (parent, "INSTALLFOLDER")}
    for relative in files:
        current = Path(".")
        for part in relative.parts[:-1]:
            previous = current
            current = current / part
            if current not in directories:
                dir_id = ident("GuideDir_", current)
                node = ET.SubElement(directories[previous][0], "Directory", Id=dir_id, Name=part)
                directories[current] = (node, dir_id)
        component_id = ident("Guide_", relative)
        component = ET.SubElement(directories[relative.parent][0], "Component", Id=component_id,
                                  Guid=str(uuid.uuid5(GUID_NAMESPACE, relative.as_posix())).upper())
        ET.SubElement(component, "File", Id=ident("GuideFile_", relative), Source=str((destination / relative).resolve()))
        ET.SubElement(component, "RegistryValue", Root="HKCU", Key=r"Software\Dutchman CAD\Dutchman Convert",
                      Name=component_id, Type="integer", Value="1", KeyPath="yes")
        ET.SubElement(group, "ComponentRef", Id=component_id)
        if relative == Path("AGENTS.md"):
            cleanup_component = component
    for relative in sorted(directories, key=lambda path: (-len(path.parts), path.as_posix())):
        if relative != Path("."):
            dir_id = directories[relative][1]
            ET.SubElement(cleanup_component, "RemoveFolder", Id=ident(f"GuideRemove_{999-len(relative.parts):03d}_", relative), Directory=dir_id, On="uninstall")
    ET.indent(wix)
    ET.ElementTree(wix).write(output, encoding="utf-8", xml_declaration=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--wix", type=Path)
    args = parser.parse_args()
    files = stage(args.destination)
    if args.wix:
        wix_fragment(args.destination, files, args.wix)


if __name__ == "__main__":
    main()
