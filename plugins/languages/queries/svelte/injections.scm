; A component's script is JavaScript, or TypeScript with `lang="ts"`; its style CSS, or
; SCSS with `lang="scss"`; `{…}` expressions and block conditions are JavaScript, each apart.

((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#not-match? @_tag "(?i)\\slang\\s*=\\s*[\"']?ts[\"'\\s>]")
 (#set! injection.language "javascript")
 (#set! injection.combined))

((script_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#match? @_tag "(?i)\\slang\\s*=\\s*[\"']?ts[\"'\\s>]")
 (#set! injection.language "typescript")
 (#set! injection.combined))

((style_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#not-match? @_tag "(?i)\\slang\\s*=\\s*[\"']?scss[\"'\\s>]")
 (#set! injection.language "css")
 (#set! injection.combined))

((style_element
   (start_tag) @_tag
   (raw_text) @injection.content)
 (#match? @_tag "(?i)\\slang\\s*=\\s*[\"']?scss[\"'\\s>]")
 (#set! injection.language "scss")
 (#set! injection.combined))

((expression (svelte_raw_text) @injection.content)
 (#set! injection.language "javascript"))

([
   (if_start condition: (svelte_raw_text) @injection.content)
   (else_if_start condition: (svelte_raw_text) @injection.content)
   (each_start identifier: (svelte_raw_text) @injection.content)
   (await_start (svelte_raw_text) @injection.content)
   (key_start (svelte_raw_text) @injection.content)
 ]
 (#set! injection.language "javascript"))
