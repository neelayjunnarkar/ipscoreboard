#!/bin/bash
cowsay -n -f $(cowsay -l | tail -n +2 | sed 's/\ /\n/g' | shuf -n 1) <<< $(echo -e "$1")
