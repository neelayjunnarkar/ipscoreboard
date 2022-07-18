import sys
import os

# This script is to convert cow files into text files that
# can be directly printed below a text bubble.
# Uses default cowsay styling.
# Example of use (the ./ is important):
# python3 cow_to_text.py ./cows/*

if len(sys.argv) < 2:
    raise ValueError('Provide cow file names to convert to text')

for i in range(1, len(sys.argv)):
    cow_file = sys.argv[i]
    cow_output = os.popen(f'cowsay -f {cow_file} " "').read()
    cow = "\n".join(cow_output.split("\n")[3:])
    cow_text_file = open(cow_file + ".txt", "w")
    cow_text_file.write(cow)
    cow_text_file.close()

