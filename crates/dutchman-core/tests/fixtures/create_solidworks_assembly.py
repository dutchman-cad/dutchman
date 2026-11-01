"""Generate an authored assembly, using cadmpeg's public synthetic envelope layout.

Recipe: cadmpeg 6118386, codec-sldprt/src/test_support/container.rs
(make_block, outer_header, native_envelope_fixture). No third-party design input.
"""
from pathlib import Path
import struct
import zlib

IDENTITY = "1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"
SHIFTED = "0 1 0 0 -1 0 0 0 0 0 1 0 0.03 0 0 1"
MIRRORED = "-1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"


def reference(key, name, transform, suppressed="NO"):
    return (f'<swReference id="{key}" swName="{name}" swReferenceNumber="1" '
            f'swModelRef="8" swSuppressed="{suppressed}" swHidden="NO" '
            f'swIsVirtualComponent="NO" swTransform="{transform}"/>')


def generate():
    references = "".join([
        reference(4, "First", IDENTITY),
        reference(7, "Second", SHIFTED),
        reference(10, "Suppressed", IDENTITY, "YES"),
        reference(11, "Mirrored", MIRRORED),
    ])
    xml = rf'''<?xml version="1.0" encoding="UTF-8"?>
<swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema" swVersion="17000">
 <swHeader>
  <swFile id="3" swDocType="ASSEMBLY" swPath="W:\authored\Assembly.SLDASM"/>
  <swFile id="9" swDocType="PART" swPath="W:\authored\SOLIDWORKS_CUBE.SLDPRT"/>
 </swHeader>
 <swModelList>
  <swModel id="8" swName="Cube" swConfigurationName="Default" swFileRef="9"/>
  <swModel id="2" swName="Assembly" swConfigurationName="Default" swFileRef="3">
   {references}
  </swModel>
 </swModelList>
 <swConfigurationList>
  <swConfiguration id="1" swName="Default" swMostRecentConfiguration="YES" swModelRef="2"/>
 </swConfigurationList>
</swSolidWorks>'''.encode()
    name = b"swXmlContents/COMPINSTANCETREE"
    swapped = bytes(((b << 4) | (b >> 4)) & 255 for b in name)
    compressor = zlib.compressobj(wbits=-15)
    compressed = compressor.compress(xml) + compressor.flush()
    header = bytes.fromhex("140006000800") + struct.pack(
        "<5I", 0x43, zlib.crc32(xml), len(compressed), len(xml), len(swapped))
    output = struct.pack("<I", 1) + struct.pack(">I", 4)
    output += bytes.fromhex("5ad9c887") + header + swapped + compressed
    central = bytes.fromhex("452b7e280000") + header + bytes(4)
    central += struct.pack("<H", 1) + bytes(4) + struct.pack("<I", 0) + swapped
    offset = len(output) - 8
    output += central + bytes.fromhex("9f825e02") + bytes(4)
    output += struct.pack("<HHIIH", 1, 1, len(central), offset, 0)
    return output


if __name__ == "__main__":
    Path(__file__).with_name("placed_cubes.sldasm").write_bytes(generate())
