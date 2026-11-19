### History

- Keep circular rims editable as fillets and equal-setback chamfers when targeting
  SolidCraft 0.44.1 or newer, including rims rebuilt with a different seam or several edges.
- Keep the selected SolidCraft patch release when converting, so feature checks and file
  extensions follow `--solidcraft-version`.
- Require exact source circle geometry and select rims only on their original body, so
  elliptical edges and coincident rims on another body cannot receive the wrong blend.
