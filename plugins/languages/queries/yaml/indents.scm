; A key's value on the lines after it is a level deeper.
[
  (block_mapping_pair)
  (flow_mapping)
  (flow_sequence)
] @indent.begin

; Their entries line up, like a mapping after `- `.
[
  (block_mapping)
  (block_sequence)
] @indent.align

["}" "]"] @indent.end

[":" "{" "["] @indent.open
