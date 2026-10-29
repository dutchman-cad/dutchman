### cadmpeg

- The cadmpeg pin moves from `e06fa52` to `2e01486`. Fusion designs whose sweeps or rotational
  supports failed cadmpeg's model checks convert now. Inventor holes are drilled the way the
  hole points (they used to replay outside the part and fall back to exact geometry), browser
  work planes are read as datum planes, and a split of a solid is read and reported as not
  mapped yet. SolidWorks parts keep their stored face senses.
