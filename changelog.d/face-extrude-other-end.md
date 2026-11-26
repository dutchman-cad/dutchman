### History

- Convert extrudes of an end face of an imported Fusion body, such as the 1515 aluminum extrusions of RailCore, when the source records the face at the opposite end of an identical profile: the extrude is written from the end that the next feature's face or the source's final shape confirms.
- Extrude a face with a hole as a ring, so the hole stays open, and find a face whose edges SolidCraft numbers as split pieces.
- Judge a face extrude that is followed by more face extrudes on the same imported body by the source result check of the last one, instead of refusing it for having no result of its own to compare.
