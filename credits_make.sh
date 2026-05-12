#!/bin/bash

cargo metadata --format-version 1 |
  jq '
    . as $m
    | ($m.resolve.nodes | map(.id) | unique) as $used
    | .packages = [$m.packages[] | select(.id as $id | $used | index($id))]
  '  | jq '
{
  crates: [
    .packages[]
    | {
        name,
        version,
        authors,
        description,
        repository,
        homepage
      }
  ]
}' > credits.json
