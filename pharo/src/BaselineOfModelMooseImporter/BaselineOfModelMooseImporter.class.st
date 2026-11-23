Class {
	#name : 'BaselineOfModelMooseImporter',
	#superclass : 'BaselineOf',
	#category : 'BaselineOfModelMooseImporter',
	#package : 'BaselineOfModelMooseImporter'
}

{ #category : 'baselines' }
BaselineOfModelMooseImporter >> baseline: spec [

	<baseline>
	spec for: #common do: [
		spec
			baseline: 'Famix'
			with: [
				spec
					repository: 'github://moosetechnology/Famix:development/src';
					loads: #( 'Minimal' 'ModelJava' ) ].
		spec
			package: 'Model-Moose-Importer'
			with: [ spec requires: #( 'Famix' ) ].
		spec group: 'default' with: #( 'Model-Moose-Importer' ) ]
]
